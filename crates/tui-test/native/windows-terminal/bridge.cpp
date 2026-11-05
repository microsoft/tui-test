// Copyright (c) Microsoft Corporation. Licensed under the MIT license.

#include "cascadia/TerminalCore/pch.h"
#include "bridge.h"
#include "cascadia/TerminalCore/Terminal.hpp"
#include "renderer/inc/DummyRenderer.hpp"
#include "terminal/parser/OutputStateMachineEngine.hpp"
#include <til/hash.h>

#include <cstring>

using Microsoft::Terminal::Core::Terminal;
using namespace Microsoft::Console::VirtualTerminal;

static_assert(sizeof(wchar_t) == sizeof(uint16_t));

namespace
{
    template<typename F>
    int boundary(wt_error* error, F&& operation) noexcept
    {
        if (error)
        {
            error->message[0] = '\0';
        }
        try
        {
            operation();
            return 0;
        }
        catch (const winrt::hresult_error& e)
        {
            if (error)
            {
                sprintf_s(error->message, "TerminalCore HRESULT 0x%08lx", static_cast<unsigned long>(e.code().value));
            }
        }
        catch (const std::exception& e)
        {
            if (error)
            {
                strncpy_s(error->message, e.what(), _TRUNCATE);
            }
        }
        catch (...)
        {
            if (error)
            {
                strcpy_s(error->message, "Unknown TerminalCore exception");
            }
        }
        return -1;
    }

    void require(bool valid, const char* message)
    {
        if (!valid)
        {
            throw std::invalid_argument(message);
        }
    }

    void dimensions(uint16_t cols, uint16_t rows)
    {
        require(cols > 0 && rows > 0 && cols <= SHRT_MAX && rows <= SHRT_MAX,
                "TerminalCore dimensions must be in 1..=32767");
    }

    uint32_t color(const TextColor value)
    {
        switch (value.GetType())
        {
        case ColorType::IsDefault:
            return 0;
        case ColorType::IsIndex16:
        case ColorType::IsIndex256:
            return 0x01000000u | value.GetIndex();
        case ColorType::IsRgb:
            return 0x02000000u | value.GetRGB();
        default:
            throw std::runtime_error("Unrecognized TerminalCore color type");
        }
    }

    struct Snapshot
    {
        wt_snapshot view{};
        std::vector<wt_cell> cells;
        std::wstring strings;

        wt_span append(const std::wstring_view text)
        {
            const wt_span result{ strings.size(), text.size() };
            strings.append(text);
            return result;
        }

        wt_snapshot* finish()
        {
            view.abi_version = WT_ABI_VERSION;
            view.cells = cells.data();
            view.cell_count = cells.size();
            view.strings = reinterpret_cast<const uint16_t*>(strings.data());
            view.string_count = strings.size();
            view.owner = this;
            return &view;
        }
    };

    struct Output
    {
        wt_output view{};
        std::wstring text;
    };
}

struct wt_terminal
{
    Terminal terminal;
    DummyRenderer renderer{ &terminal };
    std::wstring output;
    std::wstring query_output;
    std::wstring clipboard;
    uint64_t clipboard_revision = 0;
    uint64_t bells = 0;
    uint32_t scrollback = 0;
    bool querying = false;
    bool alternate = false;
    int primary_top = 0;
    int primary_cols = 0;
    DWORD thread_id = GetCurrentThreadId();

    void check_thread() const
    {
        require(thread_id == GetCurrentThreadId(), "TerminalCore accessed from a different thread");
    }

    void scroll_changed()
    {
        if (!terminal.GetBufferAndViewport().isMainBuffer)
        {
            alternate = true;
            return;
        }
        const auto leaving_alternate = std::exchange(alternate, false);
        const auto history = terminal.ViewStartIndex();
        if (history == 0)
        {
            // TerminalCore can retain a latent scroll offset after ED 3.
            terminal.UserScrollViewport(0);
        }
        else if (leaving_alternate && terminal.GetViewport().Width() == primary_cols)
        {
            terminal.UserScrollViewport(std::min(primary_top, history));
        }
        primary_top = terminal.GetViewport().Top();
        primary_cols = terminal.GetViewport().Width();
    }

    ITermDispatch& dispatch()
    {
        // Terminal::Create constructs this exact public engine type.
        return static_cast<OutputStateMachineEngine&>(terminal.GetStateMachine().Engine()).Dispatch();
    }

    template<typename F>
    std::wstring query(F&& operation)
    {
        require(!querying, "Nested TerminalCore query");
        query_output.clear();
        querying = true;
        const auto reset = wil::scope_exit([&] { querying = false; });
        // Call the dispatch API, NOT ProcessString: the parser may currently be
        // holding half an escape sequence. These calls only read native state.
        operation(dispatch());
        return query_output;
    }

    bool mode(DispatchTypes::ModeParams mode)
    {
        const auto reply = query([&](ITermDispatch& target) { target.RequestMode(mode); });
        const auto separator = reply.rfind(L';');
        require(separator != std::wstring::npos && reply.ends_with(L"$y") && separator + 4 == reply.size(),
                "TerminalCore did not return a valid DECRPM state");
        switch (reply[separator + 1])
        {
        case L'1':
        case L'3':
            return true;
        case L'2':
        case L'4':
            return false;
        default:
            throw std::runtime_error("TerminalCore cannot report a required terminal mode");
        }
    }

    uint32_t kitty_flags()
    {
        const auto reply = query([](ITermDispatch& target) { target.QueryKittyKeyboardProtocol(); });
        const auto begin = reply.find(L'?');
        require(begin != std::wstring::npos && reply.ends_with(L'u'),
                "TerminalCore did not return Kitty keyboard state");
        const auto number = reply.substr(begin + 1, reply.size() - begin - 2);
        size_t used = 0;
        const auto value = std::stoul(number, &used);
        require(used == number.size() && value <= 31, "Invalid TerminalCore Kitty keyboard flags");
        return static_cast<uint32_t>(value);
    }
};

int wt_create(const wt_config* config, wt_terminal** result, wt_error* error) noexcept
{
    if (result)
    {
        *result = nullptr;
    }
    return boundary(error, [&] {
        require(config && result, "Null TerminalCore configuration or result");
        require(config->abi_version == WT_ABI_VERSION, "TerminalCore bridge ABI version mismatch");
        dimensions(config->cols, config->rows);
        require(config->scrollback <= static_cast<uint32_t>(SHRT_MAX - config->rows),
                "TerminalCore buffer height (rows + scrollback) cannot exceed 32767");
        auto instance = std::make_unique<wt_terminal>();
        auto& core = instance->terminal;
        const auto lock = core.LockForWriting();
        core.SetWriteInputCallback([self = instance.get()](const std::wstring_view text) {
            (self->querying ? self->query_output : self->output).append(text);
        });
        core.SetWarningBellCallback([self = instance.get()] { ++self->bells; });
        core.SetTitleChangedCallback([](std::wstring_view) {});
        core.SetCopyToClipboardCallback([self = instance.get()](wil::zwstring_view text) {
            if (self->clipboard != text)
            {
                self->clipboard.assign(text);
                ++self->clipboard_revision;
            }
        });
        core.SetScrollPositionChangedCallback([self = instance.get()](int, int, int) { self->scroll_changed(); });
        core.TaskbarProgressChangedCallback([] {});
        core.SetShowWindowCallback([](bool) {});
        core.SetPlayMidiNoteCallback([](int, int, std::chrono::microseconds) {});
        core.CompletionsChangedCallback([](std::wstring_view, unsigned int) {});
        core.SetSearchMissingCommandCallback([](std::wstring_view, til::CoordType) {});
        core.SetShowNotificationCallback([](std::wstring_view, std::wstring_view) {});
        core.SetClearQuickFixCallback([] {});
        core.Create({ config->cols, config->rows }, static_cast<til::CoordType>(config->scrollback), instance->renderer);
        instance->scrollback = config->scrollback;
        instance->scroll_changed();
        auto& settings = core.GetRenderSettings();
        for (size_t i = 0; i < 256; ++i)
        {
            settings.SetColorTableEntry(i, config->colors[i + 3]);
        }
        settings.SetColorAlias(ColorAlias::DefaultForeground, TextColor::DEFAULT_FOREGROUND, config->colors[0]);
        settings.SetColorAlias(ColorAlias::DefaultBackground, TextColor::DEFAULT_BACKGROUND, config->colors[1]);
        settings.SetColorTableEntry(TextColor::CURSOR_COLOR, config->colors[2]);
        settings.SaveDefaultSettings();
        // Focus is local input state, not an OS window or clipboard connection.
        (void)core.FocusChanged(true);
        *result = instance.release();
    });
}

void wt_destroy(wt_terminal* terminal) noexcept
{
    delete terminal;
}

int wt_write(wt_terminal* instance, const uint16_t* text, size_t length, uint64_t* bells, wt_error* error) noexcept
{
    return boundary(error, [&] {
        require(instance && bells && (text || length == 0), "Invalid TerminalCore write arguments");
        instance->check_thread();
        const auto lock = instance->terminal.LockForWriting();
        if (length)
        {
            instance->terminal.Write({ reinterpret_cast<const wchar_t*>(text), length });
        }
        *bells = std::exchange(instance->bells, 0);
    });
}

int wt_resize(wt_terminal* instance, uint16_t cols, uint16_t rows, wt_error* error) noexcept
{
    return boundary(error, [&] {
        require(instance, "Null TerminalCore handle");
        instance->check_thread();
        dimensions(cols, rows);
        require(instance->scrollback <= static_cast<uint32_t>(SHRT_MAX - rows),
                "TerminalCore buffer height (rows + scrollback) cannot exceed 32767");
        const auto lock = instance->terminal.LockForWriting();
        THROW_IF_FAILED(instance->terminal.UserResize({ cols, rows }));
    });
}

int wt_scroll(wt_terminal* instance, uint32_t offset, wt_error* error) noexcept
{
    return boundary(error, [&] {
        require(instance, "Null TerminalCore handle");
        instance->check_thread();
        const auto lock = instance->terminal.LockForWriting();
        const auto history = instance->terminal.ViewStartIndex();
        instance->terminal.UserScrollViewport(history - static_cast<int>(std::min(offset, static_cast<uint32_t>(history))));
        instance->scroll_changed();
    });
}

int wt_read(wt_terminal* instance, uint32_t rows, wt_snapshot** result, wt_error* error) noexcept
{
    if (result)
    {
        *result = nullptr;
    }
    return boundary(error, [&] {
        require(instance && result && rows <= 3, "Invalid TerminalCore snapshot arguments");
        instance->check_thread();
        auto& core = instance->terminal;
        const auto lock = core.LockForReading();
        auto snapshot = std::make_unique<Snapshot>();
        auto& state = snapshot->view.state;
        const auto visible = core.GetViewport();
        const auto history = core.ViewStartIndex();
        state.cols = static_cast<uint32_t>(visible.Width());
        state.rows = static_cast<uint32_t>(visible.Height());
        state.history = static_cast<uint32_t>(history);
        state.offset = static_cast<uint32_t>(history - visible.Top());
        const auto cursor = core.GetTextBuffer().GetCursor().GetPosition();
        state.cursor_x = static_cast<uint32_t>(std::clamp(cursor.x, 0, visible.Width() - 1));
        state.cursor_y = static_cast<uint32_t>(std::clamp(cursor.y - history, 0, visible.Height() - 1));
        switch (core.GetTextBuffer().GetCursor().GetType())
        {
        case CursorType::Underscore:
        case CursorType::DoubleUnderscore:
            state.cursor_shape = 1;
            break;
        case CursorType::VerticalBar:
            state.cursor_shape = 2;
            break;
        case CursorType::Legacy:
        case CursorType::EmptyBox:
        case CursorType::FullBox:
            state.cursor_shape = 0;
            break;
        default:
            throw std::runtime_error("Unrecognized TerminalCore cursor type");
        }
        constexpr DispatchTypes::ModeParams modes[] = {
            DispatchTypes::ModeParams::DECCKM_CursorKeysMode,
            DispatchTypes::ModeParams::DECNKM_NumericKeypadMode,
            DispatchTypes::ModeParams::DECOM_OriginMode,
            DispatchTypes::ModeParams::DECAWM_AutoWrapMode,
            DispatchTypes::ModeParams::IRM_InsertReplaceMode,
            DispatchTypes::ModeParams::FOCUS_EVENT_MODE,
            DispatchTypes::ModeParams::XTERM_BracketedPasteMode,
            DispatchTypes::ModeParams::ASB_AlternateScreenBuffer,
            DispatchTypes::ModeParams::DECTCEM_TextCursorEnableMode,
        };
        for (size_t i = 0; i < std::size(modes); ++i)
        {
            if (instance->mode(modes[i]))
            {
                state.modes |= 1u << i;
            }
        }
        state.kitty_flags = instance->kitty_flags();
        const auto& settings = core.GetRenderSettings();
        state.colors[0] = settings.GetColorAlias(ColorAlias::DefaultForeground);
        state.colors[1] = settings.GetColorAlias(ColorAlias::DefaultBackground);
        state.colors[2] = settings.GetColorTableEntry(TextColor::CURSOR_COLOR);
        for (size_t i = 0; i < 256; ++i)
        {
            state.colors[i + 3] = settings.GetColorTableEntry(i);
        }
        snapshot->view.title = snapshot->append(core.GetConsoleTitle());
        snapshot->view.clipboard = snapshot->append(instance->clipboard);
        snapshot->view.clipboard_revision = instance->clipboard_revision;
        const auto first_row = rows == 1 ? visible.Top() : rows == 2 ? history : 0;
        const auto row_count = rows == 0 ? 0 : rows == 3 ? history + visible.Height() : visible.Height();
        snapshot->cells.reserve(static_cast<size_t>(row_count) * state.cols);
        std::unordered_map<uint16_t, std::pair<wt_span, wt_span>> hyperlinks;
        for (int y = first_row; y < first_row + row_count; ++y)
        {
            for (int x = 0; x < visible.Width(); ++x)
            {
                const auto native = core.GetTextBuffer().GetCellDataAt({ x, y });
                const auto& attr = native->TextAttr();
                wt_cell cell{};
                cell.text = snapshot->append(native->DbcsAttr() == DbcsAttribute::Trailing ? L"" : native->Chars());
                cell.foreground = color(attr.GetForeground());
                cell.background = color(attr.GetBackground());
                cell.underline_color = color(attr.GetUnderlineColor());
                cell.underline_style = static_cast<uint32_t>(attr.GetUnderlineStyle());
                cell.attrs =
                    (attr.IsIntense() ? 1u : 0u) |
                    (attr.IsFaint() ? 2u : 0u) |
                    (attr.IsItalic() ? 4u : 0u) |
                    (attr.IsReverseVideo() ? 8u : 0u) |
                    (attr.IsInvisible() ? 16u : 0u) |
                    (attr.IsCrossedOut() ? 32u : 0u) |
                    (attr.IsBlinking() ? 64u : 0u);
                if (const auto id = attr.GetHyperlinkId())
                {
                    const auto [it, inserted] = hyperlinks.try_emplace(id);
                    if (inserted)
                    {
                        const auto uri = core.GetHyperlinkUri(id);
                        auto custom_id = core.GetHyperlinkCustomId(id);
                        if (!custom_id.empty())
                        {
                            // TextBuffer::GetHyperlinkId appends this exact URI
                            // hash to disambiguate equal user IDs across URIs.
                            const auto suffix = L"%" + std::to_wstring(til::hash(std::wstring_view{ uri }));
                            require(custom_id.ends_with(suffix), "Unexpected TerminalCore hyperlink ID format");
                            custom_id.resize(custom_id.size() - suffix.size());
                        }
                        it->second = { snapshot->append(uri), snapshot->append(custom_id) };
                    }
                    cell.uri = it->second.first;
                    cell.link_id = it->second.second;
                }
                snapshot->cells.push_back(cell);
            }
        }
        *result = snapshot->finish();
        snapshot.release();
    });
}

void wt_snapshot_free(wt_snapshot* snapshot) noexcept
{
    if (snapshot)
    {
        delete static_cast<Snapshot*>(snapshot->owner);
    }
}

int wt_take_output(wt_terminal* instance, wt_output** result, wt_error* error) noexcept
{
    if (result)
    {
        *result = nullptr;
    }
    return boundary(error, [&] {
        require(instance && result, "Invalid TerminalCore output arguments");
        instance->check_thread();
        auto output = std::make_unique<Output>();
        output->text.swap(instance->output);
        output->view.text = reinterpret_cast<const uint16_t*>(output->text.data());
        output->view.length = output->text.size();
        output->view.owner = output.get();
        *result = &output.release()->view;
    });
}

void wt_output_free(wt_output* output) noexcept
{
    if (output)
    {
        delete static_cast<Output*>(output->owner);
    }
}

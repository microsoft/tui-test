#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/regenerate-static-media.sh [--skip-build] [--cli PATH]

Regenerates:
  static/screen.svg
  static/recording.png
  static/recording.gif
  static/recording-zoom-100.png
  static/recording-zoom-50.png
  static/recording-zoom-25.png
  static/recording-nerd-fonts.png
  static/recording-nerd-fonts.gif
  static/resize-demo.gif

static/tui-test-demo.mp4 is a manually captured monitor demo and is not changed.
EOF
}

skip_build=0
cli_path=""
while (($#)); do
    case "$1" in
        --skip-build)
            skip_build=1
            shift
            ;;
        --cli)
            if (($# < 2)); then
                echo "--cli requires a path" >&2
                exit 2
            fi
            cli_path=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/.." && pwd)
static_dir="$repo_root/static"
temp_dir=$(mktemp -d "${TMPDIR:-/tmp}/tui-test-static-media.XXXXXX")
session_prefix="static-media-$$"
sessions=()

close_session() {
    local session=$1
    local exit_code

    if [[ -z ${cli_path:-} || ! -f $cli_path ]]; then
        return
    fi

    set +e
    "$cli_path" --session "$session" close >/dev/null 2>&1
    exit_code=$?
    set -e
    if ((exit_code != 0 && exit_code != 3)); then
        echo "warning: could not close tui-test session '$session'" >&2
    fi
}

cleanup() {
    local session
    for session in "${sessions[@]}"; do
        close_session "$session"
    done
    rm -rf -- "$temp_dir"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

tui() {
    local session=$1
    shift
    "$cli_path" --session "$session" "$@"
}

register_session() {
    local session=$1
    sessions+=("$session")
    close_session "$session"
}

shell_command() {
    local script_path=$1
    printf 'bash %q' "$script_path"
}

prepare_recording_session() {
    local session=$1

    tui "$session" submit "stty -echo; clear" >/dev/null
    tui "$session" wait command --timeout 10000 >/dev/null
}

record_set() {
    local session_suffix=$1
    local cols=$2
    local rows=$3
    local demo_script=$4
    shift 4
    local outputs=("$@")
    local session="$session_prefix-$session_suffix"
    local command
    local output

    command=$(shell_command "$demo_script")
    register_session "$session"
    tui "$session" open --shell bash --cols "$cols" --rows "$rows" >/dev/null

    for output in "${outputs[@]}"; do
        prepare_recording_session "$session"
        tui "$session" record start "$static_dir/$output" --fps 20 >/dev/null
        tui "$session" submit "$command" >/dev/null
        tui "$session" expect text "done" --match first --timeout 10000 >/dev/null
        tui "$session" wait command --timeout 10000 >/dev/null
        tui "$session" record stop >/dev/null
    done

    close_session "$session"
}

record_zoom_set() {
    local session="$session_prefix-recording-zoom"
    local command
    local spec
    local output
    local zoom

    command=$(shell_command "$recording_script")
    register_session "$session"
    tui "$session" open --shell bash --cols 48 --rows 10 >/dev/null

    for spec in \
        "recording-zoom-100.png:1" \
        "recording-zoom-50.png:0.5" \
        "recording-zoom-25.png:0.25"; do
        output=${spec%%:*}
        zoom=${spec##*:}
        prepare_recording_session "$session"
        tui "$session" record start "$static_dir/$output" --fps 20 --zoom "$zoom" >/dev/null
        tui "$session" submit "$command" >/dev/null
        tui "$session" expect text "done" --match first --timeout 10000 >/dev/null
        tui "$session" wait command --timeout 10000 >/dev/null
        tui "$session" record stop >/dev/null
    done

    close_session "$session"
}

case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        binary_name=tui-test.exe
        ;;
    *)
        binary_name=tui-test
        ;;
esac

cd -- "$repo_root"

if [[ -z "$cli_path" ]]; then
    if ((skip_build == 0)); then
        echo "Building tui-test..."
        cargo build -p tui-test-cli
    fi

    target_dir=${CARGO_TARGET_DIR:-"$repo_root/target"}
    if [[ "$target_dir" != /* ]]; then
        target_dir="$repo_root/$target_dir"
    fi
    cli_path="$target_dir/debug/$binary_name"
elif [[ "$cli_path" != /* ]]; then
    cli_path="$repo_root/$cli_path"
fi

if [[ ! -f "$cli_path" ]]; then
    echo "tui-test binary not found: $cli_path" >&2
    echo "Run without --skip-build or pass --cli PATH." >&2
    exit 1
fi

mkdir -p -- "$static_dir"

screen_script="$temp_dir/screen.sh"
cat >"$screen_script" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

escape=$'\033'
bell=$'\a'
printf '%s]2;tui-test - terminal automation%s' "$escape" "$bell"
printf '%s[1;36mtui-test>%s[0m open\r\n' "$escape" "$escape"
printf '%s[32m[ok]%s[0m session ready  %s[90mBash - 60x20%s[0m\r\n\r\n' \
    "$escape" "$escape" "$escape" "$escape"
printf '%s[1;36mtui-test>%s[0m record start demo.png\r\n' "$escape" "$escape"
printf '%s[33m[rec]%s[0m recording APNG at 2x density\r\n\r\n' "$escape" "$escape"
printf '%s[1;36mtui-test>%s[0m expect text ready\r\n' "$escape" "$escape"
printf '%s[32m[ok] matched%s[0m ready\r\n' "$escape" "$escape"
sleep 30
EOF
chmod +x "$screen_script"

recording_script="$temp_dir/recording.sh"
cat >"$recording_script" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

clear
printf '\033]2;tui-test recording\a'
sleep 0.75
printf '\033[36mtui-test> \033[0m'
sleep 0.25
printf '\033[33mdemo\033[0m\n'
sleep 0.25
printf '\033[36m  tui-test terminal\033[0m\n'
sleep 0.25
printf '  record APNG / GIF / MP4\n'
sleep 0.25
printf '\033[32m  done\033[0m\n'
EOF
chmod +x "$recording_script"

nerd_font_script="$temp_dir/nerd-fonts.sh"
cat >"$nerd_font_script" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

clear
printf '\033]2;tui-test Nerd Font recording\a'
sleep 0.75
folder=$'\uf115'
powerline=$'\ue0b0'
printf '\033[36mtui-test> \033[0m'
sleep 0.25
printf '\033[33micons\033[0m\n'
sleep 0.25
printf '\033[36m  %s Nerd Font glyphs\033[0m\n' "$folder"
sleep 0.25
printf '  %s Powerline rendering\n' "$powerline"
sleep 0.25
printf '\033[32m  done\033[0m\n'
EOF
chmod +x "$nerd_font_script"

resize_script="$temp_dir/resize.sh"
cat >"$resize_script" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

clear
printf '\033]2;tui-test resize and reflow demo\a'
sleep 0.6
printf '\033[36mtui-test resize and reflow demo\033[0m\n\n'
printf '\033[33mThe terminal keeps this paragraph intact while narrower widths reflow it '
printf 'across more rows and wider widths pull it back together.\033[0m\n\n'
printf '\033[32mWatch the same words wrap, unwrap, and return to their original layout.\033[0m\n'
sleep 8
EOF
chmod +x "$resize_script"

echo "Regenerating static/screen.svg..."
screen_session="$session_prefix-screen"
register_session "$screen_session"
tui "$screen_session" run --cols 60 --rows 20 -- bash --noprofile --norc "$screen_script" >/dev/null
tui "$screen_session" expect text "recording APNG" --match first --timeout 10000 >/dev/null
tui "$screen_session" screenshot --out "$static_dir/screen.svg" >/dev/null
close_session "$screen_session"

echo "Regenerating APNG and GIF examples..."
record_set recording 48 10 "$recording_script" recording.png recording.gif

echo "Regenerating zoom comparison examples..."
record_zoom_set

echo "Regenerating Nerd Font examples..."
record_set nerd-fonts 54 12 "$nerd_font_script" \
    recording-nerd-fonts.png recording-nerd-fonts.gif

echo "Regenerating resize demo..."
resize_session="$session_prefix-resize"
register_session "$resize_session"
tui "$resize_session" open --shell bash --cols 60 --rows 16 >/dev/null
prepare_recording_session "$resize_session"
tui "$resize_session" record start "$static_dir/resize-demo.gif" --fps 20 >/dev/null
tui "$resize_session" submit "$(shell_command "$resize_script")" >/dev/null
tui "$resize_session" expect text "Watch the same words" --match first --timeout 10000 >/dev/null
sleep 0.3
tui "$resize_session" resize 42 10 >/dev/null
sleep 0.9
tui "$resize_session" resize 30 7 >/dev/null
sleep 0.9
tui "$resize_session" resize 50 12 >/dev/null
sleep 0.9
tui "$resize_session" resize 60 16 >/dev/null
tui "$resize_session" wait command --timeout 10000 >/dev/null
tui "$resize_session" record stop >/dev/null
close_session "$resize_session"

echo "Static media regenerated in $static_dir"
echo "static/tui-test-demo.mp4 is a manually captured monitor demo and was not modified."

#ifndef TUI_TEST_EXAMPLE_COMMON_H
#define TUI_TEST_EXAMPLE_COMMON_H

#ifndef _WIN32
#define _POSIX_C_SOURCE 200809L
#endif

#include "tui_test.h"
#include <stdio.h>
#include <string.h>

#ifdef _WIN32
#include <windows.h>
#else
#include <time.h>
#endif

static TuiString string_view(const char *text) {
    TuiString value = {(const uint8_t *)text, strlen(text)};
    return value;
}

static int consume(TuiResult *result) {
    if (result == NULL) {
        fputs("The library returned no result.\n", stderr);
        return 0;
    }
    int ok = result->error_kind == 0;
    if (!ok) {
        fprintf(stderr, "tui-test error %u: ", (unsigned)result->error_kind);
        if (result->error_message.len != 0) {
            fwrite(result->error_message.data, 1, result->error_message.len, stderr);
        }
        fputc('\n', stderr);
    }
    tui_result_free(result);
    return ok;
}

static int wait_for_task(TuiTask *task) {
    if (task == NULL) {
        fputs("The library returned no task.\n", stderr);
        return 0;
    }
    TuiResult *result;
    while ((result = tui_task_poll(task)) == NULL) {
        /* An event loop can do other work between polls on this same thread. */
#ifdef _WIN32
        Sleep(5);
#else
        const struct timespec delay = {0, 5000000};
        nanosleep(&delay, NULL);
#endif
    }
    tui_task_free(task);
    return consume(result);
}

static int example_main(int argc, char **argv, int asynchronous) {
    const char *greeting = "Hello from C SDK";
    if (argc == 2 && strcmp(argv[1], "--child") == 0) {
        puts(greeting);
        fflush(stdout);
        (void)getchar();
        return 0;
    }
    uint32_t version = tui_abi_version();
    if (version != ABI_VERSION) {
        fprintf(stderr, "ABI mismatch: header %u, library %u\n",
                (unsigned)ABI_VERSION, (unsigned)version);
        return 1;
    }
    printf("ABI: %u\n", (unsigned)version);

    TuiString session = string_view("{\"name\":\"c-sdk-example\"}");
    TuiString child_arg = string_view("--child");
    TuiOpenOptions options;
    memset(&options, 0, sizeof(options));
    options.wait_ready.present = true;
    options.wait_ready.value = false;
    options.recording_mode = string_view("disabled");
    int ok = consume(tui_run(session, options, string_view(argv[0]), &child_arg, 1));
    if (ok) {
        TuiQuery query = {string_view(
            "{\"nodes\":[{\"kind\":\"text\",\"text\":\"Hello from C SDK\","
            "\"occurrence\":\"any\"}],\"root\":0}")};
        TuiWaitOptions wait;
        memset(&wait, 0, sizeof(wait));
        wait.timeout_ms.present = true;
        wait.timeout_ms.value = 10000;
        ok = asynchronous
            ? wait_for_task(tui_wait_locator_start(session, query, wait))
            : consume(tui_wait_locator(session, query, wait));
        if (ok && asynchronous) {
            puts("Wait completed asynchronously.");
        }
    }
    if (ok) {
        TuiResult *result = tui_text(session, false);
        int found = 0;
        if (result != NULL && result->error_kind == 0) {
            size_t length = strlen(greeting);
            for (size_t i = 0; i + length <= result->text.len; ++i) {
                if (memcmp(result->text.data + i, greeting, length) == 0) {
                    found = 1;
                    break;
                }
            }
        }
        ok = consume(result);
        if (ok && !found) {
            fputs("The captured text does not contain the greeting.\n", stderr);
            ok = 0;
        }
        if (ok) {
            printf("Captured: %s\n", greeting);
        }
    }
    TuiOptionalBool failed = {true, !ok};
    int closed = consume(tui_close(session, failed));
    if (closed) {
        puts("Closed session.");
    }
    return ok && closed ? 0 : 1;
}

#endif

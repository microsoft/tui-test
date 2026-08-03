# Nushell ships native semantic-prompt integration (it emits VS Code's OSC 633,
# a superset of OSC 133, which shell-use understands). We just enable it and the
# OSC 7 cwd reporting, then force a minimal prompt to wait on.
$env.config = ($env.config | default {})
$env.config.shell_integration = ($env.config.shell_integration | default {})
$env.config.shell_integration.osc133 = true
$env.config.shell_integration.osc7 = true
$env.config.show_banner = false

$env.PROMPT_COMMAND = {|| "> " }
$env.PROMPT_COMMAND_RIGHT = {|| "" }
$env.PROMPT_INDICATOR = ""
$env.PROMPT_INDICATOR_VI_INSERT = ""
$env.PROMPT_INDICATOR_VI_NORMAL = ""
$env.PROMPT_MULTILINE_INDICATOR = ""


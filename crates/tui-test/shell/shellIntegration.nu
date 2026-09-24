# Keep Nushell's native semantic-prompt integration, but emit OSC 7 ourselves
# to encode literal percent signs and other URI-reserved characters in paths.
$env.config = ($env.config | default {})
$env.config.shell_integration = ($env.config.shell_integration | default {})
$env.config.shell_integration.osc133 = true
$env.config.shell_integration.osc7 = false
$env.config.show_banner = false

def __su_cwd [] {
    let path = if $nu.os-info.name == "windows" {
        $env.PWD | str replace --all '\' '/'
    } else {
        $env.PWD
    }
    let path = if ($path | str starts-with '/') { $path } else { $"/($path)" }
    $"(ansi osc)7;file://($path | url encode)(char bel)"
}

$env.PROMPT_COMMAND = {|| $"(__su_cwd)> " }
$env.PROMPT_COMMAND_RIGHT = {|| "" }
$env.PROMPT_INDICATOR = ""
$env.PROMPT_INDICATOR_VI_INSERT = ""
$env.PROMPT_INDICATOR_VI_NORMAL = ""
$env.PROMPT_MULTILINE_INDICATOR = ""

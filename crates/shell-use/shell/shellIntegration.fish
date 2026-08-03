function __su_osc; printf '\033]133;%s\007' $argv[1]; end
function __su_cwd; printf '\033]7;file://%s%s\007' $hostname $PWD; end
function __su_preexec --on-event fish_preexec; __su_osc C; end
function __su_postexec --on-event fish_postexec; __su_osc "D;$status"; end
function fish_prompt; __su_osc A; __su_cwd; printf '> '; __su_osc B; end
set -U fish_greeting
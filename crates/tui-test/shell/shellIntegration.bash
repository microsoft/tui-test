if [ -r ~/.bashrc ]; then
    . ~/.bashrc
fi
if [ -r /etc/profile ]; then
    . /etc/profile
fi
if [ -r ~/.bash_profile ]; then
    . ~/.bash_profile
elif [ -r ~/.bash_login ]; then
    . ~/.bash_login
elif [ -r ~/.profile ]; then
    . ~/.profile
fi

__su_osc() { builtin printf '\033]133;%s\007' "$1"; }
__su_cwd() { builtin printf '\033]7;file://%s%s\007' "${HOSTNAME:-}" "$PWD"; }

__su_preexec_invoke() {
    [ -n "$COMP_LINE" ] && return
    [ -z "$__su_preexec_armed" ] && return
    __su_preexec_armed=
    __su_osc "C"
}
trap '__su_preexec_invoke' DEBUG

__su_precmd() {
    local ec=$?
    if [ -n "$__su_started" ]; then
        __su_osc "D;$ec"
    fi
    __su_cwd
    PS1='\[\e]133;A\a\]> \[\e]133;B\a\]'
    __su_started=1
    __su_preexec_armed=1
}
PROMPT_COMMAND=__su_precmd
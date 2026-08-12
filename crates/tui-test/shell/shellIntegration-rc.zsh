builtin autoload -Uz add-zsh-hook

__su_zdotdir=$ZDOTDIR
if [[ -f $USER_ZDOTDIR/.zshrc ]]; then
	ZDOTDIR=$USER_ZDOTDIR
	. $USER_ZDOTDIR/.zshrc
fi
ZDOTDIR=$__su_zdotdir

__su_osc() { builtin printf '\033]133;%s\007' "$1"; }
__su_cwd() { builtin printf '\033]7;file://%s%s\007' "${HOST:-}" "$PWD"; }

__su_preexec() {
	__su_osc "C"
}

__su_precmd() {
	local ec=$?
	if [[ -n $__su_started ]]; then
		__su_osc "D;$ec"
	fi
	__su_cwd
	__su_started=1
}

setopt PROMPT_SUBST
PS1='%{$(__su_osc A)%}> %{$(__su_osc B)%}'

add-zsh-hook preexec __su_preexec
add-zsh-hook precmd __su_precmd
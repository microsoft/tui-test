if [[ -f $USER_ZDOTDIR/.zshrc ]]; then
	ZDOTDIR=$USER_ZDOTDIR
	. $USER_ZDOTDIR/.zshrc
fi

__tui_precmd() {
	PS1="> "
}
PS1="> "
add-zsh-hook precmd __tui_precmd
if [[ -f $USER_ZDOTDIR/.zshrc ]]; then
	ZDOTDIR=$USER_ZDOTDIR
	. $USER_ZDOTDIR/.zshrc
fi

PS1="%# %{%}"
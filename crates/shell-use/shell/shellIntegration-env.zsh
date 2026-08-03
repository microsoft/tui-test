__su_zdotdir=$ZDOTDIR
if [[ -f $USER_ZDOTDIR/.zshenv && $USER_ZDOTDIR != $ZDOTDIR ]]; then
	ZDOTDIR=$USER_ZDOTDIR
	. $USER_ZDOTDIR/.zshenv
fi
ZDOTDIR=$__su_zdotdir

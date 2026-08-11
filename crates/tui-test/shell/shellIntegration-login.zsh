__su_zdotdir=$ZDOTDIR
if [[ -f $USER_ZDOTDIR/.zlogin ]]; then
	ZDOTDIR=$USER_ZDOTDIR
	. $USER_ZDOTDIR/.zlogin
fi
ZDOTDIR=$__su_zdotdir

__su_zdotdir=$ZDOTDIR
if [[ -f $USER_ZDOTDIR/.zprofile ]]; then
	ZDOTDIR=$USER_ZDOTDIR
	. $USER_ZDOTDIR/.zprofile
fi
ZDOTDIR=$__su_zdotdir

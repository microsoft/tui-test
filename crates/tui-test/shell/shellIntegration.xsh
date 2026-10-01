import pathlib as __su_pathlib

$XONSH_SUPPRESS_WELCOME = True

def __su_osc(m):
    print(f'\033]133;{m}\007', end='', flush=True)

def __su_cwd():
    return f'\033]7;{__su_pathlib.Path.cwd().as_uri()}\007'

@events.on_precommand
def __su_pre(cmd, **kw):
    __su_osc('C')

@events.on_postcommand
def __su_post(cmd, rtn, **kw):
    __su_osc(f'D;{rtn}')

def __su_prompt():
    # Xonsh passes SOH/STX-bracketed escapes through without rendering or measuring them.
    # Keep them before a visible cell: prompt_toolkit drops trailing zero-width escapes.
    return f'\001\033]133;A\007{__su_cwd()}\033]133;B\007\002> '

$PROMPT = __su_prompt

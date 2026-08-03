import os as __su_os
import socket as __su_socket

$XONSH_SUPPRESS_WELCOME = True

def __su_osc(m):
    print(f'\033]133;{m}\007', end='', flush=True)

def __su_cwd():
    print(f'\033]7;file://{__su_socket.gethostname()}{__su_os.getcwd()}\007', end='', flush=True)

@events.on_precommand
def __su_pre(cmd, **kw):
    __su_osc('C')

@events.on_postcommand
def __su_post(cmd, rtn, **kw):
    __su_osc(f'D;{rtn}')

def __su_prompt():
    __su_osc('A')
    __su_cwd()
    return '> \033]133;B\007'

$PROMPT = __su_prompt

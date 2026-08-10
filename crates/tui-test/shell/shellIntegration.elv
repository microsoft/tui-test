use str

fn __su_osc {|m| print "\e]"$m"\a" }

fn __su_cwd {
  var p = (str:replace "\\" "/" $pwd)
  if (not (str:has-prefix $p "/")) { set p = "/"$p }
  __su_osc "7;file://"$p
}

fn __su_before_readline {
  __su_osc "133;A"
  __su_cwd
  __su_osc "133;B"
}

fn __su_after_readline {|_|
  __su_osc "133;C"
}

fn __su_after_command {|cmd-info|
  var status = 0
  if (not-eq $nil $cmd-info[error]) {
    set status = 1
    if (has-key $cmd-info[error] reason) {
      if (has-key $cmd-info[error][reason] exit-status) {
        set status = $cmd-info[error][reason][exit-status]
      }
    }
  }
  __su_osc "133;D;"$status
}

set edit:prompt = { put "> " }
set edit:rprompt = { put "" }
set edit:before-readline = (conj $edit:before-readline $__su_before_readline~)
set edit:after-readline = (conj $edit:after-readline $__su_after_readline~)
set edit:after-command = (conj $edit:after-command $__su_after_command~)

# Parse and compile this example; no terminal or output file is created.
Output hello.gif
Set Rows 24
Set Columns 80
Set FontFamily "JetBrains Mono"
Set FontSize 22
Set Framerate 50
Set Padding 60
Set CursorBlink true
Require echo
Hide
Env TAPE_GREETING "Hello from a tape"
Type@10ms `echo "hidden setup"` Enter
Show
Type 'echo ' '"Hello from a tape"' Enter
Set TypingSpeed 25ms
ScrollUp@100ms 2
ScrollDown 2
Sleep .5s
Hide

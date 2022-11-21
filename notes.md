render:
-> generate full frames (push)

encoder:
-> iterate over frames (pull, render is bottleneck so allow it to buffer up lots of frames)
-> generate bytes (push, io::Writer)

http:
-> send bytes back to client

import re
import sys

request_re = re.compile(r'"(HEAD|GET|POST) ([^ ]+) HTTP/1.\d"')

with open(sys.argv[1] + "_cleaned", "w") as out:
    with open(sys.argv[1], "r") as log:
        for line in log:
            s = request_re.search(line)
            if s:
                if s.group(1) == "POST":
                    continue
                else:
                    out.write(s.group(2) + "\n")
            else:
                print(line)

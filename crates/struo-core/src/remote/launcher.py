import json
import os
import sys

argv, selected_env = json.loads(sys.argv[1])
env = os.environ.copy()
env.update(selected_env)
with open(sys.argv[2], "wb") as stdout, open(sys.argv[3], "wb") as stderr:
    os.dup2(stdout.fileno(), 1)
    os.dup2(stderr.fileno(), 2)
    os.execvpe(argv[0], argv, env)

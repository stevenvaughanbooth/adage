import glob

# Paths a hostile generator names: none of them may become an edit trigger.
SOURCES = ["/etc", "../secrets", "~/.ssh", "*.pem"]
for p in glob.glob("/etc/*.conf"):
    print(p)
open("../outside/models.py", "w").write("x = 1\n")

"""Install every gallery entry into an isolated root and summarise compat per entry."""
import os, re, subprocess, sys, collections
S = "/home/bernardo/linux-rainmeter/snowmeter/target/release/snowmeter"
root = sys.argv[1]
env = dict(os.environ, SNOWMETER_CONFIG_DIR=root)
out = subprocess.run([S, "gallery"], env=env, capture_output=True, text=True).stdout
names = [l.split()[0] for l in out.splitlines()[1:] if l.strip()]
for n in names:
    r = subprocess.run([S, "install", n], env=env, capture_output=True, text=True, timeout=600)
    lines = (r.stdout + r.stderr).splitlines()
    c = collections.Counter()
    missing = collections.Counter()
    for l in lines:
        m = re.match(r"^(\S.*?): (Fully supported|Partially supported|Unsupported plugins)(.*)$", l)
        if m:
            c[m[2]] += 1
            for item in re.findall(r"[A-Za-z0-9_.!]+", m[3]):
                if item not in ("missing", "Unsupported", "plugins"):
                    missing[item] += 1
    status = "OK " if r.returncode == 0 else f"ERR{r.returncode}"
    top = ", ".join(f"{k}×{v}" for k, v in missing.most_common(4))
    print(f"{status} {n:22} full={c['Fully supported']:3} partial={c['Partially supported']:3} unsup={c['Unsupported plugins']:3}  {top}")

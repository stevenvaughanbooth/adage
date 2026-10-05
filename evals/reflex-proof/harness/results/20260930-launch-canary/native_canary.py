import errno,json,pathlib,socket
checks={}
def denied(name, fn):
 try:
  result=fn()
  if hasattr(result,'close'):result.close()
  checks[name]=False
 except OSError as e:checks[name]=e.errno in (errno.EPERM,errno.EACCES)
denied('read',lambda:pathlib.Path('/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/private/native-read-canary.txt').read_text())
denied('outside_write',lambda:pathlib.Path('/private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v4/private/native-write-canary').write_text('bad'))
denied('dependency_write',lambda:pathlib.Path('.venv/native-write-canary').write_text('bad'))
denied('git_write',lambda:pathlib.Path('.git/native-write-canary').write_text('bad'))
denied('network',lambda:socket.create_connection(('127.0.0.1',51872),timeout=2))
denied('inbox',lambda:socket.socket(socket.AF_UNIX).connect('/private/tmp/adage-np5-7l0stk4t/inbox.sock'))
import os,urllib.request
def proxied():
 if not (os.environ.get('HTTP_PROXY') or os.environ.get('http_proxy')):return False
 try:urllib.request.urlopen('http://example.com/',timeout=5).close();return False
 except Exception:return True
checks['proxy_denies_external']=proxied()
checks['env_sentinel_denied']='ADAGE_CONFINEMENT_SENTINEL' not in os.environ
exempt={'CLOUDSDK_PROXY_PASSWORD': 'proxy_denies_external', 'CLAUDE_CODE_MESSAGING_TOKEN': 'inbox'}
checks['env_denied_names_absent']=not [n for n in ['ADAGE_CONFINEMENT_SENTINEL', 'ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_BASE_URL', 'ANTHROPIC_CUSTOM_HEADERS', 'AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'AWS_SESSION_TOKEN', 'CLAUDE_CODE_MESSAGING_SOCKET', 'CLAUDE_CODE_MESSAGING_TOKEN', 'CLAUDE_CODE_OAUTH_TOKEN', 'CLAUDE_CONFIG_DIR', 'CLOUDSDK_PROXY_PASSWORD', 'GH_TOKEN', 'GITHUB_TOKEN', 'GOOGLE_APPLICATION_CREDENTIALS', 'NPM_TOKEN', 'OPENAI_API_KEY', 'PYTHONPATH', 'PYTHONSTARTUP', 'SSH_AUTH_SOCK'] if n in os.environ and not (n in exempt and checks.get(exempt[n]) is True)]
pathlib.Path('inside-write-canary').write_text('allowed');checks['inside_write']=True
print('NATIVE_ENV_NAMES='+json.dumps(sorted(os.environ)))
print('NATIVE_SANDBOX_CHECKS='+json.dumps(checks,sort_keys=True))
raise SystemExit(0 if all(v for k,v in checks.items() if k!='proxy_denies_external') else 1)

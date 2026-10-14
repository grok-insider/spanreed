#!/usr/bin/env python3
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

root = Path(os.environ['FABRIALS_LIFETIME_ROOT'])

def info(pid=None):
    pid = pid or os.getpid()
    return {'pid': pid, 'pgid': os.getpgid(pid), 'cgroup': Path(f'/proc/{pid}/cgroup').read_text(), 'cwd': os.getcwd(), 'environment': os.environ.get('FABRIALS_LIFETIME_TOKEN'), 'startTime': Path(f'/proc/{pid}/stat').read_text().rsplit(') ', 1)[1].split()[19]}

def write(name, value):
    temporary = root / (name + '.tmp')
    temporary.write_text(json.dumps(value))
    temporary.replace(root / name)

if len(sys.argv) > 1 and sys.argv[1] == 'leader':
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(str(root / 'leader.sock'))
    listener.listen()
    worker = subprocess.Popen(['sleep', '600'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    write('leader-worker.json', info(worker.pid))
    value = info()
    value['worker'] = worker.pid
    write('leader.json', value)
    while True:
        connection, _ = listener.accept()
        connection.sendall(json.dumps(value).encode())
        connection.close()

write('client.json', info())
if '--leader' in sys.argv:
    if not (root / 'leader.sock').exists():
        subprocess.Popen([sys.executable, __file__, 'leader'], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    until = time.monotonic() + 4
    while not (root / 'leader.json').exists():
        if time.monotonic() > until:
            raise TimeoutError('synthetic leader did not start')
        time.sleep(0.01)
    connection = socket.socket(socket.AF_UNIX)
    connection.connect(str(root / 'leader.sock'))
    leader = json.loads(connection.recv(8192))
    connection.close()
else:
    worker = subprocess.Popen(['sleep', '600'], stdin=subprocess.DEVNULL, stdout=None, stderr=None)
    write('worker.json', info(worker.pid))
    leader = {'worker': worker.pid}

sys.stderr.write('synthetic diagnostic\n' * 100000)
sys.stderr.flush()
for line in sys.stdin:
    message = json.loads(line)
    if message['method'] == 'never':
        write('pending.json', {'pending': True})
        continue
    result = {'protocolVersion': 1, 'agentCapabilities': {}} if message['method'] == 'initialize' else {**info(), 'leader': leader}
    print(json.dumps({'jsonrpc': '2.0', 'id': message['id'], 'result': result}), flush=True)
    if message['method'] == 'lifetime' and os.environ.get('FABRIALS_LIFETIME_EARLY_EXIT') == '1':
        break

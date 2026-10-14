import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import time
import urllib.request


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--candidate', required=True, type=Path)
    parser.add_argument('--previous', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(mode=0o700)
    state = output / 'state'
    grok = output / 'grok'
    workspace = output / 'workspace'
    grok.mkdir(mode=0o700)
    workspace.mkdir(mode=0o700)
    binary = output / 'spanreed'
    environment = dict(os.environ, FABRIALS_AGENT_STATE_DIR=str(state), GROK_HOME=str(grok), GROK_LEADER_SOCKET=str(grok / 'leader.sock'), FABRIALS_AGENT_PROGRAM=shutil.which('grok') or 'grok', GROK_DISABLE_AUTOUPDATER='1', GROK_TELEMETRY_ENABLED='false', GROK_TELEMETRY_MIXPANEL_ENABLED='false', GROK_TELEMETRY_TRACE_UPLOAD='false', GROK_FEEDBACK_ENABLED='false', OTEL_SDK_DISABLED='true', SPANREED_REMOTE_EXPERIMENT_PROJECTS='1')
    for name in ['XAI_API_KEY','GROK_API_KEY','FABRIALS_LOCAL_DEV_KEY','AI_RELAY_VIRTUAL_KEY','GROK_CODE_XAI_API_KEY']:
        environment.pop(name, None)
    report = {'schema':1,'scope':'isolated-process-lifecycle-with-local-binary-replacement','candidateSha256':sha(args.candidate),'previousSha256':sha(args.previous),'checks':{},'notQualified':['Network installer and self-update','Committed release provenance','User service-manager environment','Browser and terminal continuity'],'inferencePerformed':False}
    processes = []
    logs = []
    failure = None

    def command(*arguments, extra=None):
        result = subprocess.run([str(binary), *arguments], env={**environment, **(extra or {})}, cwd=workspace, capture_output=True, timeout=15)
        if result.returncode:
            raise RuntimeError('command_failed:' + ' '.join(arguments[:3]))
        return result.stdout.decode()

    def replace(source):
        staged = output / 'spanreed.next'
        shutil.copy2(source, staged)
        staged.chmod(0o700)
        staged.replace(binary)

    def stop(process, port):
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=5)
        try:
            connection = socket.create_connection(('127.0.0.1', port), timeout=.2)
        except OSError:
            return
        connection.close()
        raise RuntimeError('listener_survived_stop')

    def start(expected):
        log = (output / f'process-{len(processes)}.log').open('xb')
        logs.append(log)
        process = subprocess.Popen([str(binary),'remote','private-serve','https://remote-evaluation.example.ts.net'], env=environment, cwd=workspace, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        processes.append(process)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError('host_exited_before_ready')
            try:
                identity = json.loads((state / 'origin.json').read_text())
                with urllib.request.urlopen(f'http://127.0.0.1:{identity["port"]}/healthz', timeout=.5) as response:
                    health = json.load(response)
                if health.get('ok') and sha(f'/proc/{process.pid}/exe') == expected:
                    return process, identity
            except (OSError,ValueError):
                pass
            time.sleep(.05)
        raise RuntimeError('host_readiness_timeout')

    try:
        if report['candidateSha256'] == report['previousSha256']:
            raise RuntimeError('distinct_binaries_required')
        replace(args.previous)
        previous, identity = start(report['previousSha256'])
        stop(previous, identity['port'])
        replace(args.candidate)
        candidate, identity2 = start(report['candidateSha256'])
        report['checks']['identity_and_port_preserved_on_update'] = identity2 == identity
        report['checks']['private_state'] = state.stat().st_mode & 0o077 == 0 and (state / 'origin.json').stat().st_mode & 0o077 == 0
        inspected = json.loads(command('remote','doctor','--inspect','--private-origin','https://remote-evaluation.example.ts.net'))
        (output / 'doctor.json').write_text(json.dumps(inspected, indent=2))
        authorized = json.loads(command('remote','roots','authorize',str(workspace)))
        (output / 'roots-authorized.json').write_text(json.dumps(authorized, indent=2))
        listing = json.loads(command('remote','roots','list'))
        roots = listing
        if len(roots) != 1:
            raise RuntimeError('authorized_root_not_listed')
        root_id = roots[0]['rootId']
        stop(candidate, identity['port'])
        candidate, identity3 = start(report['candidateSha256'])
        report['checks']['identity_and_port_preserved_on_restart'] = identity3 == identity
        report['checks']['roots_persist_across_restart'] = json.loads(command('remote','roots','list')) == listing
        command('remote','roots','revoke',root_id)
        report['checks']['root_revocation'] = json.loads(command('remote','roots','list')) == []
        stop(candidate, identity['port'])
        replace(args.previous)
        rolled_back, identity4 = start(report['previousSha256'])
        report['checks']['identity_and_port_preserved_on_rollback'] = identity4 == identity
        stop(rolled_back, identity['port'])
        report['checks']['listeners_closed'] = True
    except Exception as error:
        failure = type(error).__name__ + ': ' + str(error)
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=5)
        for log in logs:
            log.close()
        report.update(failure=failure, ownedProcessesStopped=all(process.poll() is not None for process in processes), scriptSha256=sha(__file__))
        report['result'] = 'failed' if failure or not all(report['checks'].values()) else 'passed'
        (output / 'report.json').write_text(json.dumps(report, indent=2))
        print(json.dumps(report))
    return int(report['result'] != 'passed')


if __name__ == '__main__':
    raise SystemExit(main())

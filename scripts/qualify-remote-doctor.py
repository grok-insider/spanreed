import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import shlex
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir(mode=0o700)
    state = root / 'never-created'
    grok = root / 'grok'
    grok.mkdir(mode=0o700)
    executable = root / 'must-not-execute'
    marker = root / 'executed'
    executable.write_text('#!/bin/sh\ntouch ' + shlex.quote(str(marker)) + '\nexit 1\n')
    executable.chmod(0o700)
    base = dict(os.environ, FABRIALS_AGENT_STATE_DIR=str(state), GROK_HOME=str(grok), FABRIALS_AGENT_PROGRAM=str(executable))
    for key in ['XAI_API_KEY','GROK_API_KEY','FABRIALS_LOCAL_DEV_KEY','AI_RELAY_VIRTUAL_KEY','GROK_CODE_XAI_API_KEY','GROK_LEADER_SOCKET']:
        base.pop(key, None)
    cases = []

    def run(name, origin='https://fixture.example.ts.net', environment=None):
        result = subprocess.run([str(args.binary.resolve()),'remote','doctor','--inspect','--private-origin',origin],env={**base,**(environment or {})},capture_output=True,text=True,timeout=10)
        value = json.loads(result.stdout)
        assert result.returncode != 0
        assert value['qualified'] is False and value['readOnly'] is True
        assert not marker.exists() and not state.exists()
        checks = {item['id']:item for item in value['checks']}
        cases.append({'case':name,'exitCode':result.returncode,'report':value})
        return checks

    checks = run('missing-program',environment={'FABRIALS_AGENT_PROGRAM':str(root/'absent')})
    assert checks['grok-program']['code'] == 'grok_not_on_calling_process_path'
    checks = run('inspect-does-not-execute-found-program')
    assert checks['grok-program']['status'] == 'passed'
    assert checks['host-control']['status'] == 'failed'
    checks = run('relative-grok-home',environment={'GROK_HOME':'relative-home'})
    assert checks['grok-home']['code'] == 'grok_home_missing_or_relative'
    for origin in ['http://fixture.example.ts.net','https://fixture.example.ts.net/path','https://fixture.example.ts.net?query=1','https://fixture.example.ts.net#fragment','https://localhost','https://127.0.0.1','https://user@fixture.example.ts.net']:
        checks = run('invalid-origin:'+origin,origin=origin)
        assert checks['private-origin']['code'] == 'invalid_private_origin'
    for origin in ['https://fixture.example.ts.net','https://fixture.example.ts.net:444','https://example.com']:
        checks = run('valid-origin:'+origin,origin=origin)
        assert checks['private-origin']['status'] == 'passed'
        assert checks['private-origin']['code'] == 'exact_https_origin'
    path = grok / 'leader.sock'
    stale = socket.socket(socket.AF_UNIX)
    stale.bind(str(path))
    stale.close()
    checks = run('stale-socket-is-not-claimed-live',environment={'GROK_LEADER_SOCKET':str(path)})
    assert checks['leader-socket']['status'] == 'unverified'
    assert checks['leader-socket']['code'] == 'socket_present_liveness_unverified'
    assert checks['service-environment']['status'] == 'unverified'
    assert checks['terminal-continuity']['status'] == 'unverified'
    assert checks['browser-route']['status'] == 'unverified'
    path.unlink()
    report = {'schema':1,'scope':'real-cli-read-only-diagnostics-with-isolated-inputs','result':'passed','binarySha256':hashlib.sha256(args.binary.read_bytes()).hexdigest(),'scriptSha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'cases':cases,'notQualified':['Service environment comparison','Active leader protocol probe','Embedded terminal detection','Browser route','Successful onboarding and user utility']}
    (root/'report.json').write_text(json.dumps(report,indent=2))
    print(json.dumps({'result':report['result'],'cases':len(cases),'noProcessStarted':not marker.exists(),'stateNotCreated':not state.exists()}))


if __name__ == '__main__':
    main()

"""Public MCP tests. Every memory assertion uses the running HTTP server."""
import json
from concurrent.futures import ThreadPoolExecutor
import os
from pathlib import Path
import socket
import shutil
import resource
import signal
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request

BINARY = str(Path(sys.argv.pop(1)).resolve())

class Server:
    def __init__(self, home=None):
        self.temp = tempfile.TemporaryDirectory() if home is None else None
        self.home = Path(self.temp.name if self.temp else home)
        self.process = None
        self.logs = tempfile.TemporaryFile()
        self.token = None
        self.port = None

    def command(self, *args):
        return subprocess.run([BINARY, '--home', str(self.home), *args], capture_output=True, text=True, timeout=10)

    def init(self):
        result = self.command('init')
        assert result.returncode == 0, result.stderr
        assert self.command('scope', 'add', 'project:a').returncode == 0
        self.token = self.grant('writer', ['project:a'], True)

    def grant(self, name, scopes, write=False):
        args = ['grant', 'add', name, '--read']
        for scope in scopes:
            args += ['--scope', scope]
        if write:
            args += ['--write']
        result = self.command(*args)
        assert result.returncode == 0, result.stderr
        return result.stdout.strip()

    def start(self, pause=None, file_limit=None):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            self.port = sock.getsockname()[1]
        environment = dict(os.environ)
        if pause:
            environment['TUCK_TEST_PAUSE'] = pause
        def set_limits():
            signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
            resource.setrlimit(resource.RLIMIT_FSIZE, (file_limit, file_limit))
        self.process = subprocess.Popen([BINARY, '--home', str(self.home), 'serve', '--port', str(self.port)], stdout=self.logs, stderr=self.logs, env=environment, preexec_fn=set_limits if file_limit is not None else None)
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                self.logs.seek(0)
                raise AssertionError(self.logs.read().decode())
            try:
                with socket.create_connection(('127.0.0.1', self.port), .05):
                    return
            except OSError:
                time.sleep(.02)
        raise AssertionError('server did not start')

    def stop(self, kill=False):
        if self.process and self.process.poll() is None:
            self.process.kill() if kill else self.process.terminate()
            self.process.wait(timeout=10)

    def close(self):
        self.stop()
        self.logs.close()
        if self.temp:
            self.temp.cleanup()

    def rpc(self, method, params=None, token=None, headers=None):
        data = json.dumps({'jsonrpc':'2.0', 'id':1, 'method':method, 'params':params or {}}).encode()
        request_headers = {'Content-Type':'application/json', 'Accept':'application/json, text/event-stream', 'MCP-Protocol-Version':'2025-03-26', 'Authorization':'Bearer ' + (self.token if token is None else token)}
        request_headers.update(headers or {})
        req = urllib.request.Request(f'http://127.0.0.1:{self.port}/mcp', data, request_headers)
        with urllib.request.urlopen(req, timeout=20) as response:
            text = response.read().decode()
        if text.startswith('event:') or text.startswith('data:'):
            text = next(line[6:] for line in text.splitlines() if line.startswith('data: '))
        return json.loads(text)

    def call(self, name, args, token=None):
        response = self.rpc('tools/call', {'name':name, 'arguments':args}, token)
        if 'error' in response:
            return {'error':response['error']}
        result = response['result']
        return json.loads(result['content'][0]['text'])

    def save(self, operation='save-1', content='Use Widget::parse with snake_case.', scope='project:a', token=None):
        return self.call('memory_save', {'scope':scope, 'kind':'decision', 'content':content, 'source':{'reference':'README.md#parser', 'origin':'project_artifact'}, 'operation_id':operation}, token)

class Contract(unittest.TestCase):
    def setUp(self):
        self.server = Server()
        self.addCleanup(self.server.close)
        self.server.init()
        self.server.start()

    def test_durable_save_get_and_code_search(self):
        s = self.server
        tools = s.rpc('tools/list')['result']['tools']
        self.assertEqual({t['name'] for t in tools}, {'memory_save','memory_get','memory_search','memory_update','memory_delete'})
        saved = s.save()
        self.assertEqual(saved['revision'], 1)
        record = s.call('memory_get', {'id':saved['id']})
        self.assertEqual(record['content'], 'Use Widget::parse with snake_case.')
        self.assertEqual(record['source']['origin'], 'project_artifact')
        self.assertTrue(record['writer'])
        self.assertEqual(record['scope'], 'project:a')
        self.assertTrue(record['created_at'])
        self.assertEqual(s.call('memory_search', {'query':'Widget::parse','scopes':['project:a']})[0]['id'], saved['id'])
        s.stop()
        s.start()
        self.assertEqual(s.call('memory_get', {'id':saved['id']}), record)

    def test_correction_is_revision_checked_and_retry_safe(self):
        s = self.server
        saved = s.save()
        self.assertEqual(s.save(), saved)
        self.assertEqual(s.save(content='different')['error']['code'], 'conflict')
        update = {'id':saved['id'], 'expected_revision':1, 'content':'Use NewParser::run instead.', 'source':{'reference':'CHANGELOG.md', 'origin':'user_statement'}, 'operation_id':'edit-1'}
        updated = s.call('memory_update', update)
        self.assertEqual(updated['revision'], 2)
        self.assertEqual(s.call('memory_update', update), updated)
        stale = dict(update, operation_id='stale')
        self.assertEqual(s.call('memory_update', stale)['error']['code'], 'conflict')
        self.assertEqual(s.call('memory_search', {'query':'Widget::parse','scopes':['project:a']}), [])
        self.assertEqual(s.call('memory_search', {'query':'NewParser::run','scopes':['project:a']})[0]['revision'], 2)
        edits = [dict(update, expected_revision=2, operation_id=f'race-{i}', content=f'Race winner {i}') for i in range(2)]
        with ThreadPoolExecutor(2) as pool:
            replies = list(pool.map(lambda a: s.call('memory_update', a), edits))
        self.assertEqual(sum('error' not in r for r in replies), 1)
        self.assertEqual(sum(r.get('error', {}).get('code') == 'conflict' for r in replies), 1)
        s.stop(kill=True)
        s.start()
        self.assertEqual(s.call('memory_update', update), updated)
        self.assertEqual(s.call('memory_get', {'id':saved['id']})['revision'], 3)

    def test_forgetting_survives_old_mutation_retries(self):
        s = self.server
        saved = s.save()
        update = {'id':saved['id'], 'expected_revision':1, 'content':'Forget this replacement.', 'source':{'reference':'private-source', 'origin':'user_statement'}, 'operation_id':'edit'}
        s.call('memory_update', update)
        stale = {'id':saved['id'], 'expected_revision':1, 'operation_id':'stale-delete'}
        self.assertEqual(s.call('memory_delete', stale)['error']['code'], 'conflict')
        delete = dict(stale, expected_revision=2, operation_id='delete')
        deleted = s.call('memory_delete', delete)
        self.assertEqual(deleted['outcome'], 'deleted')
        self.assertEqual(s.call('memory_delete', delete), deleted)
        self.assertEqual(s.call('memory_get', {'id':saved['id']})['error']['code'], 'not_found')
        self.assertEqual(s.call('memory_search', {'query':'replacement','scopes':['project:a']}), [])
        s.stop(kill=True)
        s.start()
        self.assertEqual(s.call('memory_delete', delete), deleted)
        self.assertEqual(s.save()['outcome'], 'deleted')
        self.assertEqual(s.call('memory_update', update)['outcome'], 'deleted')
        self.assertNotEqual(s.save(operation='deliberate-save')['id'], saved['id'])

    def test_scopes_limits_authentication_and_ranking(self):
        s = self.server
        saved = s.save()
        self.assertEqual(s.command('scope', 'add', 'project:b').returncode, 0)
        self.assertEqual(s.command('scope', 'add', 'shared:user').returncode, 0)
        other = s.grant('other', ['project:b'], True)
        reader = s.grant('reader', ['project:a'])
        shared = s.grant('shared', ['project:a','shared:user'], True)
        s.stop()
        s.start()
        self.assertEqual(s.call('memory_get', {'id':saved['id']}, other)['error']['code'], 'not_found')
        self.assertEqual(s.call('memory_get', {'id':'absent'}, other)['error']['code'], 'not_found')
        self.assertEqual(s.save(operation='denied', token=reader)['error']['code'], 'forbidden')
        self.assertEqual(s.call('memory_search', {'query':'Widget','scopes':['project:a']}, other)['error']['code'], 'forbidden')
        shared_record = s.save('shared', 'Prefer explicit tests.', 'shared:user', shared)
        self.assertEqual(s.call('memory_get', {'id':shared_record['id']})['error']['code'], 'not_found')
        self.assertEqual(s.call('memory_search', {'query':'explicit', 'scopes':['shared:user']}, shared)[0]['id'], shared_record['id'])
        for headers, token, code in [({}, 'bad-secret', 401), ({'Host':'evil.example'}, s.token, 403), ({'Origin':'https://evil.example'}, s.token, 403)]:
            with self.assertRaises(urllib.error.HTTPError) as caught:
                s.rpc('tools/list', token=token, headers=headers)
            self.assertEqual(caught.exception.code, code)
        for content in ['', ' ', 'x'*8193, '界'*2731]:
            self.assertEqual(s.save('invalid', content)['error']['code'], 'validation')
        for query in ['', ' '*5, 'x'*1025]:
            self.assertEqual(s.call('memory_search', {'query':query, 'scopes':['project:a']})['error']['code'], 'validation')
        for limit in [0,21]:
            self.assertEqual(s.call('memory_search', {'query':'Widget', 'scopes':['project:a'], 'limit':limit})['error']['code'], 'validation')
        for query in ['" OR 1=1 --', 'nonexistent', '+++']:
            self.assertEqual(s.call('memory_search', {'query':query, 'scopes':['project:a']}), [])
        malformed = s.rpc('tools/call', {'name':'memory_save','arguments':{'kind':'not-a-kind'}})
        self.assertTrue(malformed['result']['isError'])
        self.assertEqual(json.loads(malformed['result']['content'][0]['text'])['error']['code'], 'validation')
        first = s.save('rank1', 'uniqueword secondword ' + '界'*600)
        s.save('rank2', 'uniqueword')
        for i in range(6):
            s.save('tie'+str(i), 'uniqueword')
        ranked = s.call('memory_search', {'query':'uniqueword secondword','scopes':['project:a']})
        self.assertEqual(len(ranked), 5)
        self.assertEqual(ranked[0]['id'], first['id'])
        self.assertEqual(len(ranked[0]['excerpt']), 512)
        self.assertEqual([r['id'] for r in ranked[1:]], sorted(r['id'] for r in ranked[1:]))
        self.assertEqual(s.call('memory_search', {'query':'README.md#parser','scopes':['project:a']})[0]['source']['reference'], 'README.md#parser')
        s.stop()
        s.logs.seek(0)
        diagnostics = s.logs.read().decode()
        self.assertNotIn(s.token, diagnostics)
        self.assertNotIn('Use Widget::parse', diagnostics)
        self.assertNotIn('README.md#parser', diagnostics)
        self.assertNotIn('nonexistent', diagnostics)

    def test_grant_rotation_and_recreation_keep_distinct_receipts(self):
        s = self.server
        saved = s.save()
        old_token = s.token
        rotation = s.command('grant', 'rotate', 'writer')
        self.assertEqual(rotation.returncode, 0)
        # Running process uses its immutable startup grant snapshot.
        self.assertEqual(s.save(), saved)
        s.stop()
        s.token = rotation.stdout.strip()
        s.start()
        with self.assertRaises(urllib.error.HTTPError) as caught:
            s.rpc('tools/list', token=old_token)
        self.assertEqual(caught.exception.code, 401)
        self.assertEqual(s.save(), saved)
        self.assertEqual(s.command('grant','revoke','writer').returncode, 0)
        s.token = s.grant('writer', ['project:a'], True)
        s.stop()
        s.start()
        self.assertNotEqual(s.save()['id'], saved['id'])

    def test_stopped_backup_restore_and_lock_recovery(self):
        s = self.server
        saved = s.save()
        second = s.command('serve', '--port', str(s.port))
        self.assertNotEqual(second.returncode, 0)
        self.assertIn('already in use', second.stderr)
        s.stop(kill=True)
        with tempfile.TemporaryDirectory() as parent:
            destination = Path(parent) / 'restored'
            shutil.copytree(s.home, destination)
            restored = Server(destination)
            restored.token = s.token
            self.addCleanup(restored.close)
            restored.start()
            self.assertEqual(restored.call('memory_get', {'id':saved['id']})['content'], 'Use Widget::parse with snake_case.')
            self.assertEqual(restored.save(), saved)
            restored.stop()
        s.start()
        self.assertEqual(s.call('memory_get', {'id':saved['id']})['revision'], 1)
        s.stop()
        # A future schema is fixture input. All memory assertions stay on MCP.
        with sqlite3.connect(s.home / 'memory.db') as connection:
            connection.execute('PRAGMA user_version=999')
        connection.close()
        self.assertIn('newer schema', s.command('serve').stderr)

    def test_startup_failures_preserve_store(self):
        s = self.server
        saved = s.save()
        s.stop()
        config_path = s.home / 'config.json'
        original = config_path.read_bytes()
        config_path.write_text('{broken')
        self.assertIn('Invalid config.json', s.command('serve').stderr)
        config_path.write_bytes(original)
        config_path.chmod(0o644)
        self.assertIn('must belong to you', s.command('serve').stderr)
        config_path.chmod(0o600)
        s.start()
        self.assertEqual(s.call('memory_get', {'id':saved['id']})['revision'], 1)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            s.stop()
            failure = s.command('serve', '--port', str(listener.getsockname()[1]))
            self.assertIn('Cannot bind', failure.stderr)
        s.start()
        self.assertEqual(s.save(), saved)

    @unittest.skipUnless(os.environ.get('TUCK_TEST_HOOKS') == '1', 'requires cargo test --features test-hooks')
    def test_termination_at_mutation_boundaries_is_atomic(self):
        for operation in ['save','update','delete']:
            for stage in ['record','index','before_commit','after_commit']:
                with self.subTest(operation=operation, stage=stage):
                    s = Server()
                    try:
                        s.init()
                        s.start()
                        saved = s.save('seed', 'boundary old value')
                        args = {'scope':'project:a','kind':'fact','content':'boundary new value','source':{'reference':'crash-fixture','origin':'user_statement'},'operation_id':'interrupted'}
                        if operation != 'save':
                            args = {'id':saved['id'],'expected_revision':1,'operation_id':'interrupted'}
                        if operation == 'update':
                            args.update(content='boundary new value',source={'reference':'new-source','origin':'user_statement'})
                        s.stop()
                        s.start(pause=operation+':'+stage)
                        with ThreadPoolExecutor(1) as pool:
                            future = pool.submit(s.call, 'memory_'+operation, args)
                            deadline = time.monotonic()+10
                            while time.monotonic() < deadline:
                                s.logs.seek(0)
                                if ('TUCK_TEST_PAUSED '+operation+':'+stage).encode() in s.logs.read():
                                    break
                                time.sleep(.01)
                            else:
                                self.fail('mutation pause was not reached')
                            s.stop(kill=True)
                            try:
                                future.result()
                            except (OSError, ValueError):
                                pass
                        s.start()
                        committed = stage == 'after_commit'
                        current = s.call('memory_get', {'id':saved['id']})
                        if operation == 'delete' and committed:
                            self.assertEqual(current['error']['code'], 'not_found')
                        else:
                            self.assertEqual(current['revision'], 2 if operation == 'update' and committed else 1)
                        matches = s.call('memory_search', {'query':'new','scopes':['project:a']})
                        self.assertEqual(len(matches), 1 if committed and operation != 'delete' else 0)
                        retry = s.call('memory_'+operation, args)
                        self.assertNotIn('error', retry)
                        self.assertEqual(s.call('memory_'+operation, args), retry)
                    finally:
                        s.close()

    def test_delete_update_race_and_post_delete_reads(self):
        s = self.server
        saved = s.save()
        update = {'id':saved['id'],'expected_revision':1,'operation_id':'race-update','content':'Race correction','source':{'reference':'race','origin':'user_statement'}}
        delete = {'id':saved['id'],'expected_revision':1,'operation_id':'race-delete'}
        with ThreadPoolExecutor(2) as pool:
            updating = pool.submit(s.call, 'memory_update', update)
            deleting = pool.submit(s.call, 'memory_delete', delete)
            results = [updating.result(), deleting.result()]
        self.assertEqual(sum('error' not in r for r in results), 1)
        current = s.call('memory_get', {'id':saved['id']})
        if 'error' not in current:
            self.assertEqual(current['revision'], 2)
            self.assertEqual(results[1]['error']['code'], 'conflict')
            s.call('memory_delete', dict(delete, expected_revision=2, operation_id='final-delete'))
        with ThreadPoolExecutor(4) as pool:
            replies = list(pool.map(lambda _:s.call('memory_get', {'id':saved['id']}), range(12)))
        self.assertTrue(all(r['error']['code']=='not_found' for r in replies))

    def test_current_grant_applies_to_old_receipts(self):
        s = self.server
        saved = s.save()
        s.stop()
        config_path = s.home / 'config.json'
        config = json.loads(config_path.read_text())
        config['scopes'].append('project:b')
        config['grants'][0]['scopes'] = ['project:b']
        config_path.write_text(json.dumps(config))
        s.start()
        self.assertEqual(s.save()['error']['code'], 'forbidden')
        self.assertEqual(s.call('memory_get', {'id':saved['id']})['error']['code'], 'not_found')

    def test_disk_write_failure_preserves_previous_commit(self):
        s = self.server
        saved = s.save()
        s.stop()
        try:
            s.start(file_limit=0)
        except AssertionError:
            pass
        else:
            failed = s.save('write-failure', 'This write must fail.')
            self.assertEqual(failed['error']['code'], 'storage_error')
        s.stop(kill=True)
        s.start()
        self.assertEqual(s.call('memory_get', {'id':saved['id']})['revision'], 1)
        self.assertEqual(s.save(), saved)

    def test_missing_and_corrupt_database_fail_safely(self):
        s = self.server
        saved = s.save()
        s.stop()
        database = s.home / 'memory.db'
        parked = s.home / 'memory.saved'
        database.rename(parked)
        failure = s.command('serve')
        self.assertNotEqual(failure.returncode, 0)
        self.assertIn('database', failure.stderr.lower())
        self.assertFalse(database.exists())
        parked.rename(database)
        original = database.read_bytes()
        database.write_bytes(b'not a database')
        self.assertNotEqual(s.command('serve').returncode, 0)
        self.assertEqual(database.read_bytes(), b'not a database')
        database.write_bytes(original)
        s.start()
        self.assertEqual(s.call('memory_get', {'id':saved['id']})['revision'], 1)

    def test_escaped_maximum_text_sizes_are_accepted(self):
        s = self.server
        args = {'scope':'project:a','kind':'fact','content':'\x01'*8192,
                'source':{'reference':'\x02'*2048,'origin':'user_statement'},
                'operation_id':'maximum-escaped'}
        saved = s.call('memory_save', args)
        self.assertEqual(saved['revision'], 1)
        memory = s.call('memory_get', {'id':saved['id']})
        self.assertEqual(memory['content'], args['content'])
        self.assertEqual(memory['source'], args['source'])

if __name__ == '__main__':
    unittest.main()

#!/usr/bin/env python3
"""Tiny stdio MCP server for spike S1: its one tool asks the user via elicitation/create."""
import json, os, sys, time
URL_MODE = os.environ.get('ELICIT_MODE') == 'url'
LOG = open('/tmp/ac-spike-s1/mcp-elicit.log', 'a')
def log(*a): print(time.time(), *a, file=LOG, flush=True)
def send(o): sys.stdout.write(json.dumps(o) + '\n'); sys.stdout.flush(); log('->', o)
pending = None
for line in sys.stdin:
    if not line.strip(): continue
    m = json.loads(line); log('<-', m)
    meth, mid = m.get('method'), m.get('id')
    if meth == 'initialize':
        send({'jsonrpc': '2.0', 'id': mid, 'result': {'protocolVersion': m['params'].get('protocolVersion', '2025-06-18'),
              'capabilities': {'tools': {}}, 'serverInfo': {'name': 'elic', 'version': '0'}}})
    elif meth == 'tools/list':
        send({'jsonrpc': '2.0', 'id': mid, 'result': {'tools': [{'name': 'ask_color', 'description': 'Ask the user for their favourite color.',
              'inputSchema': {'type': 'object', 'properties': {}}}]}})
    elif meth == 'tools/call':
        pending = mid
        params = ({'mode': 'url', 'message': 'Spike S1: open this page to pick your favourite color', 'url': 'https://example.com/spike-s1',
                   'elicitationId': 'spike-s1-url-1'} if URL_MODE else
                  {'message': 'Spike S1: what is your favourite color?',
                   'requestedSchema': {'type': 'object', 'properties': {'color': {'type': 'string'}}, 'required': ['color']}})
        send({'jsonrpc': '2.0', 'id': 'elicit-1', 'method': 'elicitation/create', 'params': params})
    elif mid == 'elicit-1' and pending is not None:
        send({'jsonrpc': '2.0', 'id': pending, 'result': {'content': [{'type': 'text', 'text': 'elicitation answer: ' + json.dumps(m.get('result') or m.get('error'))}]}})
        pending = None
    elif meth == 'ping':
        send({'jsonrpc': '2.0', 'id': mid, 'result': {}})
    elif mid is not None and meth is not None:
        send({'jsonrpc': '2.0', 'id': mid, 'error': {'code': -32601, 'message': 'no'}})

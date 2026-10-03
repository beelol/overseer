#!/usr/bin/env python3
"""One synthetic system-speech attempt, independent of in-memory synthesis."""
import json, math, struct, subprocess, sys, wave
from pathlib import Path
from time import monotonic
out = Path('diagnostic'); out.mkdir(exist_ok=True)
started = monotonic()
result = {'phrase': 'On it. Telling Phone to wait.', 'voice': 'system default', 'attempts': 1}
try:
    p = subprocess.run(['say', '-o', str(out / 'comparison.wav'), '--file-format=WAVE', '--data-format=LEI16@16000', '--', result['phrase']], capture_output=True, text=True, timeout=90)
    result.update(exit_code=p.returncode, stdout=p.stdout, stderr=p.stderr, elapsed_ms=(monotonic()-started)*1000)
    if p.returncode != 0: raise RuntimeError('say failed')
    with wave.open(str(out/'comparison.wav'), 'rb') as w:
        result.update(channels=w.getnchannels(), rate=w.getframerate(), width=w.getsampwidth(), frames=w.getnframes())
        samples=struct.unpack('<' + 'h'*(w.getnframes()*w.getnchannels()), w.readframes(w.getnframes()))
        result['peak'] = max((abs(x)/32768 for x in samples), default=0)
    assert result['frames'] > 16000 and result['peak'] > 0.05, result
except Exception as e:
    result['error'] = str(e)
finally:
    (out/'say.json').write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result, indent=2))
sys.exit(1 if 'error' in result else 0)

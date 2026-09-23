"""Считает, сколько кадров теперь влезает в лимит тела (gzip и plain).

Не часть отчёта, а разовая проверка цифр в README: `LIMIT / (bytes per frame)`.
"""

import gzip
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import measure  # noqa: E402

LIMIT = measure.MAX_BODY_BYTES

for label, gen in (("rotsplit", measure.gen_rot_split), ("benign", measure.gen_benign)):
    script = gen(3600)
    raw = script.to_json().encode()
    packed = gzip.compress(raw, 9, mtime=0)
    for kind, payload in (("gzip", packed), ("plain", raw)):
        per = len(payload) / 3600
        fits = int(LIMIT / per)
        print(
            f"{label:9} {kind:5}: {len(payload):>7} B / 3600 = {per:5.2f} B/кадр "
            f"-> {fits:>7} кадров = {fits / 60 / 60:5.2f} мин"
        )

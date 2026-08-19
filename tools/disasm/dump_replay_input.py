import sqlite3, struct, sys

db = r"C:\Users\dreagledr\AppData\Local\drmod\runs.db"
conn = sqlite3.connect(db)
c = conn.cursor()

# последний record run
c.execute("SELECT id, mission_id, started_at, frame_count FROM replay_runs WHERE kind='record' ORDER BY id DESC LIMIT 1")
run = c.fetchone()
print("run:", run)
if not run:
    sys.exit(0)
run_id = run[0]

c.execute("SELECT frame_index, input_unit FROM replay_record_frames WHERE replay_id=? ORDER BY frame_index", (run_id,))
rows = c.fetchall()

# InputUnit layout (x86, 40 bytes): buttons_down(u32) pressed(u32) released(u32) alternated(u32)
# left_stick(f32x2) right_stick(f32x2) left_trigger(f32) right_trigger(f32) valid(i32) repeat(i32)
def parse(buf):
    down, pressed, released, alt = struct.unpack_from("<IIII", buf, 0)
    lx, ly, rx, ry = struct.unpack_from("<ffff", buf, 16)
    lt, rt = struct.unpack_from("<ff", buf, 32)
    valid, repeat = struct.unpack_from("<ii", buf, 40)
    return down, pressed, lx, ly, rx, ry, valid

prev = None
for fi, buf in rows:
    down, pressed, lx, ly, rx, ry, valid = parse(buf)
    # печатаем только кадры с интересными битами
    if down or pressed or prev is not None and down != prev[0]:
        print(f"f={fi:4d} down={down:08X} pressed={pressed:08X} L=({lx:8.1f},{ly:8.1f}) R=({rx:8.1f},{ry:8.1f}) valid={valid}")
        prev = (down, pressed)
conn.close()

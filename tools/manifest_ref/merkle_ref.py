"""Reference implementation of the parangsae dataset manifest v2.1 hashing recipe.
Run from the repository root: python3 merkle_ref.py. Prints the golden values the Rust builder must reproduce."""
import hashlib, struct
FIX = "fixtures/vision/"  # run from the repo root
H = lambda b: hashlib.sha256(b).digest()
def s(x): b = x.encode(); return struct.pack(">I", len(b)) + b
i64 = lambda v: struct.pack(">q", v); u64 = lambda v: struct.pack(">Q", v); u32 = lambda v: struct.pack(">I", v)
def ticks(t):
    neg = t.startswith('-'); t = t.lstrip('-'); ip, _, fp = t.partition('.')
    assert len(fp) <= 8 or set(fp[8:]) <= {'0'}
    v = int(ip) * 10**8 + int((fp + '0'*8)[:8]); return -v if neg else v

KLINE_COLS = [("symbol","utf8"),("interval_us","i64"),("open_time_us","i64"),("close_time_us","i64"),
 ("open_ticks","i64"),("high_ticks","i64"),("low_ticks","i64"),("close_ticks","i64"),("volume_ticks","i64"),
 ("quote_volume_ticks","i64"),("count","i64"),("taker_buy_volume_ticks","i64"),("taker_buy_quote_volume_ticks","i64"),
 ("event_ts_us","i64"),("available_ts_us","i64"),("receipt","receipt"),("schema_version","u32"),("tick_domain","utf8"),
 ("zero_volume_flat","bool")]
def schema_hash(dataset_id, cols, rules):
    # rules: (row_encoding_version, available_ts_rules_version, gap_rules_version, conflict_rules_version)
    b = s("parangsae/schema/v2") + s(dataset_id) + u32(len(cols))
    for n, t in cols: b += s(n) + s(t)
    b += s("parangsae.scale=100000000")
    for x in rules: b += s(x)
    return H(b)
def enc_receipt(v):
    # Archive: kind byte only (fetched_at_run excluded). Live: kind byte + i64 received_ts_us.
    kind, ts = v
    return b"\x00" if kind == "archive" else b"\x01" + i64(ts)
def enc(t, v): return {"utf8": s, "i64": i64, "u32": u32, "bool": lambda x: bytes([1 if x else 0]), "receipt": enc_receipt}[t](v)

def kline_rows(path, schema_version):
    rows = []
    for line in open(path):
        f = line.strip().split(',')
        if not f[0].isdigit(): continue
        o = int(f[0]) * 1000; c = int(f[6]) * 1000
        rows.append(dict(symbol="BTCUSDT", interval_us=60_000_000, open_time_us=o, close_time_us=c,
          open_ticks=ticks(f[1]), high_ticks=ticks(f[2]), low_ticks=ticks(f[3]), close_ticks=ticks(f[4]),
          volume_ticks=ticks(f[5]), quote_volume_ticks=ticks(f[7]), count=int(f[8]),
          taker_buy_volume_ticks=ticks(f[9]), taker_buy_quote_volume_ticks=ticks(f[10]),
          event_ts_us=o, available_ts_us=c + 1000, receipt=("archive", None), schema_version=schema_version, tick_domain="price"))
        r = rows[-1]
        r["zero_volume_flat"] = (r["volume_ticks"] == 0 and r["count"] == 0 and
            r["open_ticks"] == r["high_ticks"] == r["low_ticks"] == r["close_ticks"])
    return rows  # file order; content_hash rejects disorder

def content_hash(sh, rows, cols):
    pks = [r["open_time_us"] for r in rows]
    if any(b <= a for a, b in zip(pks, pks[1:])): raise ValueError("PK not strictly increasing")
    b = s("parangsae/rows/v2") + sh + u64(len(rows))
    for r in rows:
        for n, t in cols: b += enc(t, r[n])
    return H(b)
def gaps_digest(gaps):
    if len(set(gaps)) != len(gaps): raise ValueError("duplicate gap")
    gs = sorted(gaps, key=lambda g: (g[1], g[2], g[0].encode()))
    b = s("parangsae/gaps/v1") + u32(len(gs))
    for k, st, en in gs: b += s(k) + i64(st) + i64(en)
    return H(b)
def conflicts_digest(cs):
    # cs: (pk_tuple_of_i64, class, stored_text, rejected_text); texts are the JSON-decoded strings, exact UTF-8 bytes
    ents = sorted({(tuple(pk), cls, H(st.encode()), H(rj.encode())) for pk, cls, st, rj in cs})
    b = s("parangsae/conflicts/v1") + u32(len(ents))
    for pk, cls, hs, hr in ents:
        b += u32(len(pk)) + b"".join(i64(x) for x in pk) + s(cls) + hs + hr
    return H(b)
def sources_digest(srcs):
    ss = sorted(set(srcs), key=lambda x: (x[0].encode(), bytes.fromhex(x[1])))
    b = s("parangsae/sources/v1") + u32(len(ss))
    for url, sha in ss:
        d = bytes.fromhex(sha); assert len(d) == 32; b += s(url) + d
    return H(b)
def kline_grid_gaps(rows, day_start):
    present = {r["open_time_us"] for r in rows}; gaps = []; run = None
    for i in range(1440):
        slot = day_start + i * 60_000_000
        if slot in present:
            if run: gaps.append(("grid", run[0], run[1] + 60_000_000)); run = None
        else: run = (run[0], slot) if run else (slot, slot)
    if run: gaps.append(("grid", run[0], run[1] + 60_000_000))
    return gaps
def leaf_hash(dataset_id, symbol, date, day_start, sh, rows, ch, gd, cd, sd):
    pk = [r["open_time_us"] for r in rows] or [day_start]   # zero-row leaf: pk_min = pk_max = day_start
    p = s(dataset_id) + s(symbol) + s(date) + sh + u64(len(rows)) + i64(min(pk)) + i64(max(pk)) + ch + gd + cd + sd
    return H(b"\x00" + s("parangsae/merkle/v1") + p)
def node(l, r): return H(b"\x01" + l + r)
def mth(leaves):
    if not leaves: return H(b"")
    if len(leaves) == 1: return leaves[0]
    k = 1
    while k * 2 < len(leaves): k *= 2
    return node(mth(leaves[:k]), mth(leaves[k:]))

def bundle_hash(entries):
    # entries: (dataset_id, version_root bytes, manifest_digest bytes); sorted by dataset_id bytes, unique
    ents = sorted(entries); assert len({e[0] for e in ents}) == len(ents)
    b = s("parangsae/bundle/v1") + u32(len(ents))
    for d, root, md in ents: b += s(d) + root + md
    return H(b)

if __name__ == "__main__":
    DS = "binance-vision.um.um-kline.BTCUSDT"
    RULES = ("rows/v2", "available_ts_rules/v0.2", "gap_rules/v1", "conflict_rules/v2")
    sh = schema_hash(DS, KLINE_COLS, RULES); print("schema_hash", sh.hex())
    leaves = []
    for fn, sv, date, day0 in [("um_kline_2021-06-01_headerless.csv", 1, "2021-06-01", 1622505600000000),
                              ("um_kline_2026-10-20_header.csv", 2, "2026-10-20", 1792454400000000)]:
        rows = kline_rows(FIX + fn, sv); ch = content_hash(sh, rows, KLINE_COLS)
        g = kline_grid_gaps(rows, day0); gd = gaps_digest(g); cd = conflicts_digest([]); sd = sources_digest([])
        lf = leaf_hash(DS, "BTCUSDT", date, day0, sh, rows, ch, gd, cd, sd); leaves.append(lf)
        print(date, "rows", len(rows), "gaps", g, "\n  content", ch.hex(), "\n  leaf", lf.hex())
    print("root(2 fixture leaves)", mth(leaves).hex())
    # zero-row leaf for 2021-06-02 (missing day)
    d2 = 1622592000000000
    z = leaf_hash(DS, "BTCUSDT", "2021-06-02", d2, sh, [], content_hash(sh, [], KLINE_COLS),
                  gaps_digest([("missing_file", d2, d2 + 86_400_000_000)]), conflicts_digest([]), sources_digest([]))
    print("zero-row leaf 2021-06-02", z.hex())
    print("valid 2-day version root 2021-06-01..2021-06-02", mth([leaves[0], z]).hex())
    # non-empty side sets on the 2021-06-01 leaf
    rows = kline_rows(FIX + "um_kline_2021-06-01_headerless.csv", 1)
    cd = conflicts_digest([((1622505600000000,), "schema_only_by_log", "open=1 schema=1", "open=1 schema=2")])
    sd = sources_digest([("https://data.binance.vision/data/futures/um/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2021-06-01.zip",
                          "fc949a2cdb72753faafbed1eb93a3a1847b3ba77056b6360a3a87e93a9f37141")])
    print("conflicts_digest(1 entry)", cd.hex()); print("sources_digest(1 entry)", sd.hex())
    print("leaf 2021-06-01 with sides", leaf_hash(DS, "BTCUSDT", "2021-06-01", 1622505600000000, sh, rows,
          content_hash(sh, rows, KLINE_COLS), gaps_digest(kline_grid_gaps(rows, 1622505600000000)), cd, sd).hex())
    # tree vectors over synthetic leaves L_i = H(str("leaf") || u32(i))
    L = [H(s("leaf") + u32(i)) for i in range(5)]
    for n in (0, 1, 2, 3, 5): print(f"MTH(n={n})", mth(L[:n]).hex())
    # receipt: a Live row hashes differently from the same Archive row; Archive run id never enters the hash
    live = [dict(x, receipt=("live", 1792454400123456)) for x in rows]
    print("content 2021-06-01 if rows were Live (received_ts=1792454400123456)", content_hash(sh, live, KLINE_COLS).hex())
    # tamper: +1 tick on first open
    rows[0]["open_ticks"] += 1
    print("tampered content", content_hash(sh, rows, KLINE_COLS).hex())
    try: content_hash(sh, list(reversed(rows)), KLINE_COLS); print("BUG: disorder accepted")
    except ValueError: print("disorder rejected: ok")

    root = mth([leaves[0], z])  # the valid 2-day version root
    print("bundle(1 entry, manifest_digest=32 zero bytes)", bundle_hash([("binance-vision.um.um-kline.BTCUSDT", root, bytes(32))]).hex())

    # canonical manifest (JCS) for the valid 2-day version; all scalars are strings, so sorted-key
    # compact json.dumps equals RFC 8785 here (ASCII keys, no numbers)
    import json
    def part(date, day0, rows, gaps, ch, lf):
        pk = [r["open_time_us"] for r in rows] or [day0]; av = [r["available_ts_us"] for r in rows] or [day0]
        return {"date": date, "path": f"parquet/um-kline/symbol=BTCUSDT/date={date}/part.parquet",
                "row_count": str(len(rows)), "pk_min_us": str(min(pk)), "pk_max_us": str(max(pk)),
                "available_ts_min_us": str(min(av)), "available_ts_max_us": str(max(av)), "content_hash": ch.hex(),
                "sources": [], "open_gaps": [{"kind": k, "start_us": str(a), "end_us": str(b)} for k, a, b in gaps],
                "conflicts": [], "excluded_windows": [], "gaps_digest": gaps_digest(gaps).hex(),
                "conflicts_digest": conflicts_digest([]).hex(), "sources_digest": sources_digest([]).hex(), "leaf_hash": lf.hex()}
    r1 = kline_rows(FIX + "um_kline_2021-06-01_headerless.csv", 1)
    p1 = part("2021-06-01", 1622505600000000, r1, kline_grid_gaps(r1, 1622505600000000), content_hash(sh, r1, KLINE_COLS), leaves[0])
    p2 = part("2021-06-02", d2, [], [("missing_file", d2, d2 + 86_400_000_000)], content_hash(sh, [], KLINE_COLS), z)
    man = {"spec": "parangsae/dataset-manifest/v2.1", "dataset_id": DS, "version_root": root.hex(),
           "schema": {"schema_hash": sh.hex(), "columns": [[n, t] for n, t in KLINE_COLS],
                      "excluded_from_hash": ["receipt.Archive.fetched_at_run"], "scale": "100000000",
                      "contract_version": "data-contract v0.2",
                      "rule_versions": {"row_encoding": RULES[0], "available_ts_rules": RULES[1], "gap_rules": RULES[2], "conflict_rules": RULES[3]}},
           "range": {"first_date": "2021-06-01", "last_date": "2021-06-02"},
           "partitions": [p1, p2], "summary": {"partitions": "2", "rows": str(len(r1))}}
    mb = json.dumps(man, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    print("canonical manifest 2-day version: bytes", len(mb), "manifest_digest", H(mb).hex())
    print("bundle(1 entry, real manifest_digest)", bundle_hash([(DS, root, H(mb))]).hex())

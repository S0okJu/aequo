#!/usr/bin/env python3
"""Create a synthetic Aequo DB, deterministic for a fixed schema and input size."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sqlite3

EPOCH = 1794913200


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--feeds", type=int, default=100)
    parser.add_argument("--items", type=int, default=10000)
    parser.add_argument("--schema", type=Path, help="schema.rs source; default: current worktree")
    args = parser.parse_args()
    if args.feeds < 1 or args.items < 0:
        parser.error("feeds must be positive and items nonnegative")
    if args.output.exists():
        parser.error("output already exists; use a fresh fixture path")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    schema_path = args.schema or Path(__file__).resolve().parents[2] / "src-tauri/src/db/schema.rs"
    schema = schema_path.read_text()
    sql = re.search(r'const SCHEMA: &str = r#"(.*?)"#;', schema, re.S).group(1)
    conn = sqlite3.connect(args.output)
    conn.executescript(sql)
    # Production migrations/search indexing are deliberately left to the app.
    # Dates remain within the retention window; polling/icons are not due.
    for feed in range(1, args.feeds + 1):
        conn.execute(
            "INSERT INTO feeds(id,url,title,format,next_fetch_at,last_checked_at,created_at) "
            "VALUES(?,?,?,'rss2',4102444800,?,?)",
            (feed, f"https://feed-{feed}.invalid/rss", f"테스트 Feed {feed:03d}", EPOCH, EPOCH),
        )
        conn.execute("INSERT INTO feed_icons(feed_id,mime,data,fetched_at) VALUES(?,'',X'',4102444800)", (feed,))
    for item in range(1, args.items + 1):
        paragraph = f"<p>측정용 글 {item}. Synthetic article content &amp; searchable text.</p>"
        summary = paragraph * (64 if item % 10 == 0 else 8)
        body = f"<article><h2>Article {item}</h2>{paragraph * 64}</article>"
        conn.execute(
            "INSERT INTO items(id,feed_id,guid_hash,title,link,summary,full_content,"
            "published_at,fetched_at,is_read,is_saved) VALUES(?,?,?,?,?,?,?,?,?,0,?)",
            (item, (item - 1) % args.feeds + 1, f"fixture-{item:05d}",
             f"성능 측정 Article {item:05d}", f"https://article.invalid/{item}",
             summary, body, EPOCH - (args.items - item) * 60, EPOCH, int(item % 20 == 0)),
        )
    conn.commit()
    assert conn.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
    conn.close()
    metadata = {
        "version": 1, "feeds": args.feeds, "items": args.items, "epoch": EPOCH,
        "schemaSha256": hashlib.sha256(schema.encode()).hexdigest(),
        "sha256": hashlib.sha256(args.output.read_bytes()).hexdigest(),
        "bytes": args.output.stat().st_size,
        "scope": "S0/S1 text-list fixture; no live network, images, or AI transcripts",
    }
    args.output.with_suffix(".json").write_text(json.dumps(metadata, indent=2) + "\n")
    print(json.dumps(metadata))


if __name__ == "__main__":
    main()

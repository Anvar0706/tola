//! Self-contained lightweight HTML5 Web Block Explorer and node status dashboard.

pub fn render_explorer_html(chain_id: u64) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>TOLA Network — Testnet Explorer & Dashboard</title>
  <style>
    :root {{
      --bg: #0d1117;
      --card-bg: #161b22;
      --border: #30363d;
      --text: #c9d1d9;
      --text-muted: #8b949e;
      --primary: #58a6ff;
      --accent: #2ea043;
      --warn: #d29922;
    }}
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      background: var(--bg);
      color: var(--text);
      line-height: 1.5;
      padding: 24px;
    }}
    header {{
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding-bottom: 24px;
      border-bottom: 1px solid var(--border);
      margin-bottom: 24px;
    }}
    .logo {{
      font-size: 24px;
      font-weight: 700;
      color: #fff;
      display: flex;
      align-items: center;
      gap: 12px;
    }}
    .badge {{
      background: rgba(88, 166, 255, 0.15);
      color: var(--primary);
      padding: 4px 10px;
      border-radius: 12px;
      font-size: 13px;
      font-weight: 600;
    }}
    .grid {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
      gap: 16px;
      margin-bottom: 24px;
    }}
    .card {{
      background: var(--card-bg);
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 16px;
    }}
    .card-title {{
      font-size: 13px;
      color: var(--text-muted);
      text-transform: uppercase;
      margin-bottom: 8px;
    }}
    .card-value {{
      font-size: 24px;
      font-weight: 700;
      color: #fff;
    }}
    .search-box {{
      background: var(--card-bg);
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 20px;
      margin-bottom: 24px;
    }}
    .input-group {{
      display: flex;
      gap: 12px;
    }}
    input[type="text"] {{
      flex: 1;
      background: #0d1117;
      border: 1px solid var(--border);
      color: #fff;
      padding: 10px 14px;
      border-radius: 6px;
      font-size: 14px;
    }}
    input[type="text"]:focus {{
      outline: none;
      border-color: var(--primary);
    }}
    button {{
      background: var(--primary);
      color: #fff;
      border: none;
      padding: 10px 20px;
      border-radius: 6px;
      font-weight: 600;
      cursor: pointer;
      transition: opacity 0.2s;
    }}
    button:hover {{ opacity: 0.9; }}
    pre {{
      background: #0d1117;
      border: 1px solid var(--border);
      border-radius: 6px;
      padding: 16px;
      font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
      font-size: 13px;
      overflow-x: auto;
      max-height: 400px;
    }}
    .live-dot {{
      width: 8px;
      height: 8px;
      background: var(--accent);
      border-radius: 50%;
      display: inline-block;
      margin-right: 6px;
    }}
  </style>
</head>
<body>
  <header>
    <div class="logo">
      <span class="live-dot"></span>
      TOLA Network
      <span class="badge">Chain ID: {chain_id} (Testnet)</span>
    </div>
    <div>
      <a href="/health" style="color: var(--primary); margin-right: 16px; text-decoration: none;">/health</a>
      <a href="/metrics" style="color: var(--primary); text-decoration: none;">/metrics</a>
    </div>
  </header>

  <div class="grid">
    <div class="card">
      <div class="card-title">Current Block Height</div>
      <div class="card-value" id="val-height">--</div>
    </div>
    <div class="card">
      <div class="card-title">Connected P2P Peers</div>
      <div class="card-value" id="val-peers">--</div>
    </div>
    <div class="card">
      <div class="card-title">Node Status</div>
      <div class="card-value" style="color: var(--accent);" id="val-status">Online</div>
    </div>
    <div class="card">
      <div class="card-title">Block Finality</div>
      <div class="card-value">Jolteon 2-Chain</div>
    </div>
  </div>

  <div class="search-box">
    <h3 style="margin-bottom: 12px; font-size: 16px;">Blockchain Query Explorer</h3>
    <div class="input-group">
      <input type="text" id="query-input" placeholder="Search by Block Number (e.g. 1), Block/Tx Hash (0x...), or Address (0x... / ttola1...)">
      <button onclick="runQuery()">Search</button>
    </div>
    <div id="result-container" style="margin-top: 16px; display: none;">
      <h4 style="margin-bottom: 8px; font-size: 14px; color: var(--text-muted);">Result</h4>
      <pre id="query-result"></pre>
    </div>
  </div>

  <script>
    async function updateStatus() {{
      try {{
        const res = await fetch('/health');
        if (res.ok) {{
          const data = await res.json();
          document.getElementById('val-height').innerText = data.height;
          document.getElementById('val-peers').innerText = data.peers;
          document.getElementById('val-status').innerText = 'Online';
        }}
      }} catch (e) {{
        document.getElementById('val-status').innerText = 'Offline';
      }}
    }}

    async function rpcCall(method, params = []) {{
      const res = await fetch('/', {{
        method: 'POST',
        headers: {{ 'Content-Type': 'application/json' }},
        body: JSON.stringify({{
          jsonrpc: '2.0',
          method: method,
          params: params,
          id: 1
        }})
      }});
      return await res.json();
    }}

    async function runQuery() {{
      const val = document.getElementById('query-input').value.trim();
      if (!val) return;

      const container = document.getElementById('result-container');
      const out = document.getElementById('query-result');
      container.style.display = 'block';
      out.innerText = 'Querying...';

      try {{
        if (/^\d+$/.test(val)) {{
          const hexNum = '0x' + parseInt(val, 10).toString(16);
          const r = await rpcCall('eth_getBlockByNumber', [hexNum, true]);
          out.innerText = JSON.stringify(r.result || r.error, null, 2);
        }} else if (val.startsWith('0x') && val.length === 66) {{
          const rBlock = await rpcCall('eth_getBlockByHash', [val, true]);
          if (rBlock.result) {{
            out.innerText = JSON.stringify(rBlock.result, null, 2);
          }} else {{
            const rTx = await rpcCall('eth_getTransactionByHash', [val]);
            out.innerText = JSON.stringify(rTx.result || rTx.error, null, 2);
          }}
        }} else {{
          const bal = await rpcCall('eth_getBalance', [val, 'latest']);
          const nonce = await rpcCall('eth_getTransactionCount', [val, 'latest']);
          out.innerText = JSON.stringify({{
            address: val,
            balance: bal.result,
            transaction_count: nonce.result
          }}, null, 2);
        }}
      }} catch (e) {{
        out.innerText = 'Error: ' + e.message;
      }}
    }}

    updateStatus();
    setInterval(updateStatus, 3000);
  </script>
</body>
</html>"#
    )
}

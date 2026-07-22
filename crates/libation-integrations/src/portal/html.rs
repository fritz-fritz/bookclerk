//! Minimal portal HTML (no SPA framework).

#[must_use]
pub fn landing_page(portal_base: &str, allow_abs_login: bool) -> String {
    let abs_form = if allow_abs_login {
        r#"
<section class="card">
  <h2>Sign in with Audiobookshelf</h2>
  <p>Return later to manage or revoke store connections.</p>
  <form id="abs-login">
    <label>Username <input name="username" required autocomplete="username"></label>
    <label>Password <input name="password" type="password" required autocomplete="current-password"></label>
    <button type="submit">Sign in</button>
  </form>
</section>
"#
        .to_string()
    } else {
        String::new()
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Libation Connect</title>
<style>
  :root {{ color-scheme: light dark; --bg: #1a1b1e; --fg: #e8e6e3; --muted: #9a9690; --accent: #3d8bfd; --card: #25262b; }}
  @media (prefers-color-scheme: light) {{
    :root {{ --bg: #f4f3f0; --fg: #1a1b1e; --muted: #5c5852; --card: #fff; }}
  }}
  body {{ font-family: "Iowan Old Style", "Palatino Linotype", Palatino, Georgia, serif;
    margin: 0; background: radial-gradient(1200px 600px at 10% -10%, #2a3344, var(--bg));
    color: var(--fg); min-height: 100vh; }}
  main {{ max-width: 32rem; margin: 0 auto; padding: 2.5rem 1.25rem; }}
  h1 {{ font-size: 1.75rem; letter-spacing: -0.02em; margin: 0 0 0.35rem; }}
  .lead {{ color: var(--muted); margin: 0 0 1.75rem; }}
  .card {{ background: var(--card); border-radius: 12px; padding: 1.25rem 1.35rem; margin-bottom: 1rem;
    box-shadow: 0 8px 30px rgba(0,0,0,.18); }}
  label {{ display: block; margin: 0.65rem 0; font-size: 0.95rem; }}
  input {{ width: 100%; box-sizing: border-box; margin-top: 0.25rem; padding: 0.55rem 0.65rem;
    border-radius: 8px; border: 1px solid #555; background: transparent; color: inherit; }}
  button {{ margin-top: 0.75rem; padding: 0.55rem 1rem; border: 0; border-radius: 8px;
    background: var(--accent); color: #fff; font: inherit; cursor: pointer; }}
  .err {{ color: #ff8e8e; margin-top: 0.75rem; white-space: pre-wrap; }}
  a {{ color: var(--accent); }}
  #app[hidden] {{ display: none !important; }}
  ul {{ padding-left: 1.1rem; }}
</style>
</head>
<body>
<main>
  <h1>Libation Connect</h1>
  <p class="lead">Link Audible or Libro.fm accounts. Liberated books stay when you revoke.</p>

  <div id="gate">
    <section class="card">
      <h2>Claim ticket</h2>
      <p>Use a ticket issued when your Audiobookshelf user was created.</p>
      <form id="ticket-form">
        <label>Ticket <input name="ticket" required autocomplete="off" spellcheck="false"></label>
        <button type="submit">Continue</button>
      </form>
    </section>
    {abs_form}
    <p class="err" id="gate-err" hidden></p>
  </div>

  <div id="app" hidden>
    <section class="card">
      <h2>Signed in</h2>
      <p id="who"></p>
      <button type="button" id="logout">Sign out</button>
    </section>
    <section class="card">
      <h2>Bookstore sources</h2>
      <div id="sources"></div>
    </section>
    <section class="card">
      <h2>Connections</h2>
      <ul id="connections"></ul>
    </section>
    <p class="err" id="app-err" hidden></p>
  </div>
</main>
<script>
const BASE = {base_json};
function api(path, opts={{}}) {{
  return fetch(BASE + path, Object.assign({{ credentials: 'same-origin', headers: {{ 'Content-Type': 'application/json' }} }}, opts))
    .then(async r => {{
      const text = await r.text();
      let data = null;
      try {{ data = text ? JSON.parse(text) : null; }} catch {{}}
      if (!r.ok) throw new Error((data && data.error) || text || r.statusText);
      return data;
    }});
}}
function showErr(el, msg) {{ el.hidden = !msg; el.textContent = msg || ''; }}
async function enterApp() {{
  document.getElementById('gate').hidden = true;
  document.getElementById('app').hidden = false;
  const me = await api('/api/me');
  document.getElementById('who').textContent = (me.provider || '') + ' / ' + (me.label || me.external_user_id);
  await refreshSources();
  await refreshConnections();
}}
async function refreshSources() {{
  const data = await api('/api/sources');
  const root = document.getElementById('sources');
  root.innerHTML = '';
  for (const s of data.sources || []) {{
    const wrap = document.createElement('div');
    wrap.style.marginBottom = '1rem';
    if (s.id === 'libro') {{
      wrap.innerHTML = `<h3>Libro.fm</h3>
        <form data-libro>
          <label>Email <input name="email" type="email" required></label>
          <label>Password <input name="password" type="password" required></label>
          <button type="submit">Connect Libro.fm</button>
        </form>`;
      wrap.querySelector('form').addEventListener('submit', async (e) => {{
        e.preventDefault();
        showErr(document.getElementById('app-err'), '');
        const fd = new FormData(e.target);
        try {{
          await api('/api/libro/login', {{ method: 'POST', body: JSON.stringify({{
            email: fd.get('email'), password: fd.get('password')
          }})}});
          await refreshConnections();
        }} catch (err) {{ showErr(document.getElementById('app-err'), err.message); }}
      }});
    }} else if (s.id === 'audible') {{
      wrap.innerHTML = `<h3>Audible</h3>
        <button type="button" data-audible>Start Audible login</button>
        <p><a id="audible-link" href="#" target="_blank" rel="noopener" hidden>Open login</a></p>`;
      wrap.querySelector('[data-audible]').addEventListener('click', async () => {{
        showErr(document.getElementById('app-err'), '');
        try {{
          const res = await api('/api/audible/start', {{ method: 'POST', body: '{{}}' }});
          const a = wrap.querySelector('#audible-link');
          a.href = res.url;
          a.hidden = false;
          a.textContent = 'Open Audible login';
        }} catch (err) {{ showErr(document.getElementById('app-err'), err.message); }}
      }});
    }}
    root.appendChild(wrap);
  }}
}}
async function refreshConnections() {{
  const data = await api('/api/connections');
  const ul = document.getElementById('connections');
  ul.innerHTML = '';
  for (const c of data.connections || []) {{
    const li = document.createElement('li');
    li.textContent = c.source + ' — ' + (c.label || c.account_id) + ' [' + (c.connection_status || 'active') + '] ';
    if (c.connection_status !== 'revoked') {{
      const btn = document.createElement('button');
      btn.type = 'button';
      btn.textContent = 'Revoke';
      btn.addEventListener('click', async () => {{
        if (!confirm('Revoke store credentials? Liberated books are kept.')) return;
        await api('/api/connections/' + encodeURIComponent(c.account_id) + '/revoke', {{ method: 'POST', body: '{{}}' }});
        await refreshConnections();
      }});
      li.appendChild(btn);
    }}
    ul.appendChild(li);
  }}
  if (!(data.connections || []).length) {{
    ul.innerHTML = '<li>No store connections yet.</li>';
  }}
}}
document.getElementById('ticket-form').addEventListener('submit', async (e) => {{
  e.preventDefault();
  showErr(document.getElementById('gate-err'), '');
  const ticket = new FormData(e.target).get('ticket');
  try {{
    await api('/api/redeem', {{ method: 'POST', body: JSON.stringify({{ ticket }}) }});
    await enterApp();
  }} catch (err) {{ showErr(document.getElementById('gate-err'), err.message); }}
}});
const absForm = document.getElementById('abs-login');
if (absForm) {{
  absForm.addEventListener('submit', async (e) => {{
    e.preventDefault();
    showErr(document.getElementById('gate-err'), '');
    const fd = new FormData(e.target);
    try {{
      await api('/api/login/integration', {{ method: 'POST', body: JSON.stringify({{
        provider: 'audiobookshelf',
        username: fd.get('username'),
        password: fd.get('password')
      }})}});
      await enterApp();
    }} catch (err) {{ showErr(document.getElementById('gate-err'), err.message); }}
  }});
}}
document.getElementById('logout').addEventListener('click', async () => {{
  await api('/api/logout', {{ method: 'POST', body: '{{}}' }});
  location.reload();
}});
const params = new URLSearchParams(location.search);
if (params.get('ticket')) {{
  document.querySelector('#ticket-form [name=ticket]').value = params.get('ticket');
}}
api('/api/me').then(() => enterApp()).catch(() => {{}});
</script>
</body>
</html>
"##,
        abs_form = abs_form,
        base_json = serde_json::to_string(portal_base).unwrap_or_else(|_| "\"\"".into()),
    )
}

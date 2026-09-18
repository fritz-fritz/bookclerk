from pathlib import Path

import pytest

from bookclerk_plugin_sdk.tools import (
    check_plugin,
    env_properties_for,
    fmt_plugin_toml,
    format_manifest,
    generate_types,
    package_plugin,
    sync_embed,
    validate_plugin_id,
)

ROOT = Path(__file__).resolve().parents[3]
FIXTURES = ROOT / "crates" / "bookclerk-plugin-abi" / "fixtures" / "tools"
ECHO_PY = ROOT / "examples" / "plugins-echo-workerd-python"


def test_check_valid_workerd():
    msg = check_plugin(FIXTURES / "valid-workerd")
    assert "echo_workerd_tools" in msg


def test_check_invalid_outbound():
    with pytest.raises(ValueError, match="domains"):
        check_plugin(FIXTURES / "invalid-outbound-no-domains")


def test_check_valid_logo_url():
    msg = check_plugin(FIXTURES / "valid-logo-url")
    assert "logo_url" in msg


def test_check_valid_logo_path():
    msg = check_plugin(FIXTURES / "valid-logo-path")
    assert "logo_path" in msg


def test_check_rejects_logo_javascript():
    with pytest.raises(ValueError, match="logo"):
        check_plugin(FIXTURES / "invalid-logo-javascript")


def test_check_rejects_logo_vbscript():
    with pytest.raises(ValueError, match="logo"):
        check_plugin(FIXTURES / "invalid-logo-vbscript")


def test_check_rejects_logo_parent():
    with pytest.raises(ValueError, match="logo"):
        check_plugin(FIXTURES / "invalid-logo-parent")


def test_check_rejects_native_with_domains():
    with pytest.raises(ValueError, match="only valid for runtime"):
        check_plugin(FIXTURES / "invalid-native-with-domains")


@pytest.mark.parametrize("padded", [" echo", "echo "])
def test_validate_plugin_id_rejects_whitespace(padded: str):
    with pytest.raises(ValueError, match="whitespace"):
        validate_plugin_id(padded)


@pytest.mark.parametrize("name", ["valid-native", "valid-workerd"])
def test_fmt_check_gold(name):
    gold = FIXTURES / name / "plugin.fmt.toml"
    assert "ok" in fmt_plugin_toml(gold, check_only=True)


def test_check_echo_python_workerd():
    msg = check_plugin(ECHO_PY)
    assert "echo_workerd_python" in msg


def test_package_python_vendors_sdk_package(tmp_path: Path):
    out = tmp_path / "dist"
    archive = package_plugin(ECHO_PY, out)
    assert archive.is_file()
    import tarfile

    with tarfile.open(archive, "r:gz") as tar:
        names = tar.getnames()
    assert any(n.endswith("bookclerk_plugin_sdk/workerd.py") for n in names)
    assert any(n.endswith("modules/plugin.py") for n in names)


def test_package_refuses_module_symlink_without_outside_bytes(tmp_path: Path):
    import tarfile

    from bookclerk_plugin_sdk.path_guard import copy_tree_no_symlinks

    plugin = tmp_path / "plugin"
    modules = plugin / "modules"
    modules.mkdir(parents=True)
    (plugin / "plugin.toml").write_text(
        (ECHO_PY / "plugin.toml").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    (modules / "plugin.py").write_text(
        (ECHO_PY / "modules" / "plugin.py").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    outside = tmp_path / "outside.txt"
    outside.write_text("SECRET_OUTSIDE_BYTES", encoding="utf-8")
    leak = modules / "leak.txt"
    leak.symlink_to(outside)

    dest = tmp_path / "copy"
    with pytest.raises(ValueError, match="symlink"):
        copy_tree_no_symlinks(modules, dest)

    out = tmp_path / "dist"
    with pytest.raises(ValueError, match="symlink"):
        package_plugin(plugin, out)
    assert not any(out.glob("*.tar.gz"))
    assert "SECRET_OUTSIDE_BYTES" not in "".join(
        p.read_text(encoding="utf-8", errors="ignore")
        for p in out.rglob("*")
        if p.is_file()
    )


def test_package_refuses_intermediate_dir_symlink(tmp_path: Path):
    plugin = tmp_path / "plugin"
    modules = plugin / "modules"
    modules.mkdir(parents=True)
    (plugin / "plugin.toml").write_text(
        (ECHO_PY / "plugin.toml").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    (modules / "plugin.py").write_text(
        (ECHO_PY / "modules" / "plugin.py").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    outside = tmp_path / "outside_mods"
    outside.mkdir()
    (outside / "x.py").write_text("x = 1\n", encoding="utf-8")
    (modules / "vendor").symlink_to(outside)
    out = tmp_path / "dist"
    with pytest.raises(ValueError, match="symlink"):
        package_plugin(plugin, out)


def test_package_allows_symlinked_plugin_root(tmp_path: Path):
    real = tmp_path / "real"
    modules = real / "modules"
    modules.mkdir(parents=True)
    (real / "plugin.toml").write_text(
        (ECHO_PY / "plugin.toml").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    (modules / "plugin.py").write_text(
        (ECHO_PY / "modules" / "plugin.py").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    link = tmp_path / "link"
    link.symlink_to(real)
    out = tmp_path / "dist"
    archive = package_plugin(link, out)
    assert archive.is_file()


def test_refuse_symlink_allows_symlinked_trusted_root(tmp_path: Path):
    from bookclerk_plugin_sdk.path_guard import refuse_symlink_path, resolve_under

    real = tmp_path / "real"
    real.mkdir()
    (real / "child.txt").write_text("ok\n", encoding="utf-8")
    link = tmp_path / "link"
    link.symlink_to(real)
    child = resolve_under(link, "child.txt")
    # Root may be a symlink; child must not be.
    assert refuse_symlink_path(link, child) == child


def test_refuse_symlink_blocks_bookclerk_dir_link(tmp_path: Path):
    from bookclerk_plugin_sdk.path_guard import refuse_symlink_path, resolve_under
    from bookclerk_plugin_sdk.sparse_workerd.config import materialize_config

    plugin = tmp_path / "plugin"
    modules = plugin / "modules"
    modules.mkdir(parents=True)
    (plugin / "plugin.toml").write_text(
        (ECHO_PY / "plugin.toml").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    (modules / "plugin.py").write_text(
        (ECHO_PY / "modules" / "plugin.py").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    outside = tmp_path / "outside_dir"
    outside.mkdir()
    (plugin / ".bookclerk").symlink_to(outside)
    bookclerk = resolve_under(plugin, ".bookclerk")
    with pytest.raises(ValueError, match="symlink"):
        refuse_symlink_path(plugin, bookclerk)
    with pytest.raises(ValueError, match="symlink"):
        materialize_config(
            plugin,
            __import__("tomllib").loads((plugin / "plugin.toml").read_text(encoding="utf-8")),
            listen_port=0,
            bridge_token="token",
        )
    assert not (outside / "bridge.js").exists()


def test_refuse_symlink_blocks_main_module_link(tmp_path: Path):
    from bookclerk_plugin_sdk.sparse_workerd.config import materialize_config
    import tomllib

    plugin = tmp_path / "plugin"
    modules = plugin / "modules"
    modules.mkdir(parents=True)
    (plugin / "plugin.toml").write_text(
        (ECHO_PY / "plugin.toml").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    outside = tmp_path / "outside_main.py"
    outside.write_text("# outside\n", encoding="utf-8")
    (modules / "plugin.py").symlink_to(outside)
    with pytest.raises(ValueError, match="symlink"):
        materialize_config(
            plugin,
            tomllib.loads((plugin / "plugin.toml").read_text(encoding="utf-8")),
            listen_port=0,
            bridge_token="token",
        )


def test_sync_embed_optional_vendor(tmp_path: Path):
    staging = tmp_path / "plugin"
    staging.mkdir()
    (staging / "plugin.toml").write_text(
        (ECHO_PY / "plugin.toml").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    modules = staging / "modules"
    modules.mkdir()
    (modules / "plugin.py").write_text(
        (ECHO_PY / "modules" / "plugin.py").read_text(encoding="utf-8"),
        encoding="utf-8",
    )
    assert "synced" in sync_embed(staging)
    assert (modules / "bookclerk_plugin_sdk" / "workerd.py").is_file()
    assert "ok" in check_plugin(staging)


def test_format_manifest_emits_sealed_and_loopback_tables():
    text = format_manifest(
        {
            "api_version": 3,
            "id": "echo",
            "runtime": "native",
            "command": "./echo",
            "entrypoints": ["cli"],
            "secrets": {"binding": "SECRETS"},
            "oauth": {"binding": "OAUTH"},
            "capabilities": {"network": {"mode": "deny"}},
        }
    )
    assert "\n[secrets]\n" in text
    assert "\n[oauth]\n" in text
    assert 'binding = "SECRETS"' in text
    assert 'binding = "OAUTH"' in text


def test_env_properties_include_sealed_and_loopback_bindings(tmp_path: Path):
    names = [
        name
        for name, _, _ in env_properties_for({"id": "echo", "secrets": {}, "oauth": {}})
    ]
    assert "SECRETS" in names
    assert "OAUTH" in names
    (tmp_path / "plugin.toml").write_text(
        format_manifest(
            {
                "api_version": 3,
                "id": "echo",
                "runtime": "native",
                "command": "./echo",
                "entrypoints": ["cli"],
                "secrets": {"binding": "SECRETS"},
                "oauth": {"binding": "OAUTH"},
                "capabilities": {"network": {"mode": "deny"}},
            }
        ),
        encoding="utf-8",
    )
    msg = generate_types(tmp_path)
    stub = (tmp_path / "bookclerk_configuration.py").read_text(encoding="utf-8")
    assert "wrote" in msg
    assert "SECRETS: dict[str, str]" in stub
    assert "OAUTH: Any" in stub
    assert "[secrets]" in stub
    assert "[oauth]" in stub

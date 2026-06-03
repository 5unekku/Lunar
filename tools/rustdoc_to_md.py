#!/usr/bin/env python
"""generate a single-file api reference for the lunar engine, suitable for ai context.

covers all surface-tier crates (lunar, lunar-plugin-*, lunar-pathfinding-rt),
scoped to each crate's prelude — the types a game dev actually calls.

usage:
  python rustdoc_to_md.py <project_root> <output_file> [--force]

example:
  python tools/rustdoc_to_md.py . llms.md
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from collections.abc import Iterable, Mapping
from pathlib import Path

# rewrite internal crate paths in code examples to their public lunar:: equivalents,
# longest prefix first to avoid partial matches (e.g. lunar_plugin_ before lunar_)
IMPORT_REWRITES: list[tuple[str, str]] = [
	# short lib names used in doc examples (before generic lunar_ entries)
	("lunar_animation::",    "lunar::animation::"),
	("lunar_camera_3d::",    "lunar::camera3d::"),
	("lunar_dialogue::",     "lunar::dialogue::"),
	("lunar_localization::", "lunar::localization::"),
	("lunar_particles::",    "lunar::particles::"),
	("lunar_physics_2d::",   "lunar::physics2d::"),
	("lunar_physics_3d::",   "lunar::physics3d::"),
	("lunar_spline::",       "lunar::spline::"),
	("lunar_tilemap::",      "lunar::tilemap::"),
	("lunar_timeline::",     "lunar::timeline::"),
	("lunar_zones::",        "lunar::zones::"),
	("lunar_ui::",           "lunar::ui::"),
	("lunar_ai::",           "lunar::ai::"),
	# long package names (lunar-plugin-X → lunar_plugin_X)
	("lunar_plugin_animation::", "lunar::animation::"),
	("lunar_plugin_camera_3d::", "lunar::camera3d::"),
	("lunar_plugin_dialogue::",  "lunar::dialogue::"),
	("lunar_plugin_localization::","lunar::localization::"),
	("lunar_plugin_particles::", "lunar::particles::"),
	("lunar_plugin_physics_2d::","lunar::physics2d::"),
	("lunar_plugin_physics_3d::","lunar::physics3d::"),
	("lunar_plugin_spline::",    "lunar::spline::"),
	("lunar_plugin_tilemap::",   "lunar::tilemap::"),
	("lunar_plugin_timeline::",  "lunar::timeline::"),
	("lunar_plugin_zones::",     "lunar::zones::"),
	("lunar_plugin_ui::",        "lunar::ui::"),
	("lunar_plugin_ai::",        "lunar::ai::"),
	("lunar_pathfinding_rt::",   "lunar::pathfinding::"),
	# core deps re-exported at lunar root or prelude
	("lunar_math::",             "lunar::"),
	("lunar_core::",             "lunar::"),
	("lunar_render::",           "lunar::"),
	("lunar_input::",            "lunar::"),
	("lunar_assets::",           "lunar::"),
	("lunar_gamedata::",         "lunar::"),
	("lunar_2d::",               "lunar::"),
	("lunar_3d::",               "lunar::"),
	# bevy_ecs
	("bevy_ecs::prelude",        "lunar::prelude"),
	("bevy_ecs::",               "lunar::"),
]

# bare crate names appearing in prose without :: (e.g. `lunar_render` in a doc comment).
# matched with word boundaries so lunar_render doesn't corrupt lunar_render_3d etc.
_BARE_CRATE_REWRITES: list[tuple[re.Pattern[str], str]] = [
	(re.compile(r"\blunar_plugin_\w+\b"), "lunar"),
	(re.compile(r"\blunar_pathfinding_rt\b"), "lunar::pathfinding"),
	(re.compile(r"\blunar_math\b"),      "lunar"),
	(re.compile(r"\blunar_render_3d\b"), "lunar"),
	(re.compile(r"\blunar_render\b"),    "lunar"),
	(re.compile(r"\blunar_core\b"),      "lunar"),
	(re.compile(r"\blunar_input\b"),     "lunar"),
	(re.compile(r"\blunar_assets\b"),    "lunar"),
	(re.compile(r"\blunar_gamedata\b"),  "lunar"),
	(re.compile(r"\blunar_2d\b"),        "lunar"),
	(re.compile(r"\blunar_3d\b"),        "lunar"),
	(re.compile(r"\bbevy_ecs\b"),        "lunar"),
]


# strip rustdoc intra-crate links like [`Type`](crate::module::Type) → `Type`
_INTRA_LINK_RE = re.compile(r'\[(`[^`]+`)\]\(crate::[^\)]+\)')


def rewrite_crate_refs(line: str) -> str:
	"""rewrite internal crate paths to their public lunar:: equivalents.

	applied to both use statements in code blocks and inline backtick references
	in prose (e.g. `lunar_render::Sprite` → `lunar::Sprite`).
	"""
	# strip rustdoc intra-crate links first so crate:: paths don't leak
	if "crate::" in line:
		line = _INTRA_LINK_RE.sub(r'\1', line)
		# any remaining bare crate:: (e.g. in inline code) — drop the prefix
		line = line.replace("crate::", "")
	for old, new in IMPORT_REWRITES:
		if old in line:
			line = line.replace(old, new)
	for pattern, new in _BARE_CRATE_REWRITES:
		if pattern.search(line):
			line = pattern.sub(new, line)
	return line


KIND_TITLES = {
	"module": "Modules",
	"struct": "Structs",
	"enum": "Enums",
	"trait": "Traits",
	"function": "Functions",
	"constant": "Constants",
	"static": "Statics",
	"type_alias": "Type Aliases",
	"trait_alias": "Trait Aliases",
	"macro": "Macros",
}

FACADE_PACKAGE = "lunar-lib"
SURFACE_PACKAGES = frozenset({"lunar-lib", "lunar-pathfinding-rt"})
SURFACE_PREFIXES = ("lunar-plugin-",)

# core dep crates that feed the "## core api" section, in output order.
# each is processed from its own rustdoc json.
CORE_DEP_PACKAGES: list[tuple[str, str]] = [
	# (package_name, display_name)
	("lunar-core",    "core"),
	("lunar-math",    "math"),
	("lunar-render",  "render"),
	("lunar-input",   "input"),
	("lunar-assets",  "assets"),
	("lunar-gamedata","gamedata"),
	("lunar-2d",      "2d"),
	("lunar-3d",      "3d"),
]

CORE_DEP_NAMES = frozenset(pkg for pkg, _ in CORE_DEP_PACKAGES)


def is_surface_package(name: str) -> bool:
	return (
		name in SURFACE_PACKAGES
		or name in CORE_DEP_NAMES
		or any(name.startswith(p) for p in SURFACE_PREFIXES)
	)


def is_plugin_package(name: str) -> bool:
	return any(name.startswith(p) for p in SURFACE_PREFIXES)


def load_json(path: Path) -> dict:
	return json.loads(path.read_text(encoding="utf-8"))


def item_kind(item: Mapping) -> str:
	return next(iter(item["inner"].keys()))


def is_public(item: Mapping) -> bool:
	return item.get("visibility") == "public"


def filter_docs(docs: str, heading_offset: int = 0, strip_headings: bool = False) -> list[str]:
	"""filter rustdoc to allowed sections.

	heading_offset: prepend this many extra '#' chars to in-doc headings.
	strip_headings: if True, omit heading lines entirely (use for item-level docs
	  to avoid blowing up the document heading hierarchy).
	"""
	lines = docs.rstrip().splitlines()
	kept: list[str] = []
	current_section = "summary"
	in_code_fence = False
	allowed_sections = {
		"summary",
		"quick start",
		"usage",
		"example",
		"examples",
		"arguments",
		"returns",
		"errors",
		"panics",
	}
	for line in lines:
		stripped = line.strip()
		if stripped.startswith("```"):
			in_code_fence = not in_code_fence
		if not in_code_fence and stripped.startswith("#"):
			section_name = stripped.lstrip("#").strip().lower()
			current_section = section_name
			if section_name in allowed_sections and not strip_headings:
				kept.append("#" * heading_offset + line)
			continue
		if current_section not in allowed_sections:
			continue
		kept.append(rewrite_crate_refs(line))
	return kept


def render_docs(
	lines: list[str],
	docs: str | None,
	heading_offset: int = 0,
	strip_headings: bool = False,
) -> None:
	if not docs:
		return
	filtered = filter_docs(docs, heading_offset, strip_headings)
	if not filtered:
		return
	lines.append("")
	lines.extend(filtered)


def reexport_targets(items: Mapping[str, Mapping], item_ids: Iterable[int]) -> list[Mapping]:
	targets = []
	seen: set[str] = set()
	for item_id in item_ids:
		item = items[str(item_id)]
		if not is_public(item):
			continue
		if "use" not in item["inner"]:
			continue
		target_id = item["inner"]["use"].get("id")
		if target_id is None:
			continue
		target_id_str = str(target_id)
		if target_id_str in seen:
			continue
		seen.add(target_id_str)
		target = items.get(target_id_str)
		if target:
			targets.append(target)
	return targets


def render_items_by_kind(
	lines: list[str],
	items: Mapping[str, Mapping],
	item_ids: Iterable[int],
	heading_level: int,
	extra_items: Iterable[Mapping] | None = None,
	allowed_names: set[str] | None = None,
) -> None:
	by_kind: dict[str, list[Mapping]] = {}

	def maybe_add(item: Mapping) -> None:
		name = item.get("name")
		if allowed_names is not None and name not in allowed_names:
			return
		kind = item_kind(item)
		if kind != "use":
			by_kind.setdefault(kind, []).append(item)

	for item_id in item_ids:
		item = items[str(item_id)]
		if is_public(item):
			maybe_add(item)

	if extra_items:
		for item in extra_items:
			maybe_add(item)

	for kind, title in KIND_TITLES.items():
		if kind not in by_kind:
			continue
		lines.append("")
		lines.append("#" * heading_level + f" {title}")
		for item in sorted(by_kind[kind], key=lambda i: i.get("name") or ""):
			name = item.get("name") or "(anonymous)"
			lines.append("")
			lines.append("#" * (heading_level + 1) + f" {name}")
			# strip headings from item docs — they'd nest at wrong levels
			render_docs(lines, item.get("docs"), strip_headings=True)


def find_child_module(items: Mapping[str, Mapping], parent_id: int, name: str) -> int | None:
	parent = items.get(str(parent_id))
	if not parent:
		return None
	for child_id in parent["inner"]["module"].get("items", []):
		child = items.get(str(child_id))
		if child and child.get("name") == name and "module" in child["inner"]:
			return child_id
	return None


def render_surface_items(
	lines: list[str],
	items: Mapping[str, Mapping],
	root_id: int,
	heading_level: int,
	allowed_names: set[str] | None = None,
) -> None:
	"""render a crate's surface api — prelude if present, else top-level public items."""
	root_children = items[str(root_id)]["inner"]["module"].get("items", [])
	prelude_id = find_child_module(items, root_id, "prelude")
	if prelude_id is not None:
		prelude_children = items[str(prelude_id)]["inner"]["module"].get("items", [])
		reexported = reexport_targets(items, prelude_children)
		render_items_by_kind(lines, items, prelude_children, heading_level, reexported, allowed_names)
	else:
		reexported = reexport_targets(items, root_children)
		render_items_by_kind(lines, items, root_children, heading_level, reexported, allowed_names)


def prelude_name_filter(facade_data: dict) -> dict[str, set[str] | None]:
	"""build per-crate name allowlists from the facade prelude's use items.

	returns {crate_pkg_name: set_of_names} for specific imports,
	or {crate_pkg_name: None} for glob imports (meaning: use the crate's own prelude).
	crates absent from the result have no filter (include all public items).
	"""
	items = facade_data["index"]
	root_id = int(facade_data["root"])
	prelude_id = find_child_module(items, root_id, "prelude")
	if prelude_id is None:
		return {}

	result: dict[str, set[str] | None] = {}
	for child_id in items[str(prelude_id)]["inner"]["module"].get("items", []):
		child = items.get(str(child_id))
		if not child:
			continue
		if "use" not in child["inner"]:
			continue
		use_inner = child["inner"]["use"]
		source = use_inner.get("source") or ""
		name = use_inner.get("name") or ""
		is_glob = use_inner.get("is_glob", False)

		parts = source.split("::")
		if not parts or not parts[0]:
			continue
		# normalize rust crate name (underscores) to package name (hyphens)
		crate_pkg = parts[0].replace("_", "-")

		if is_glob:
			# glob: defer to the crate's own prelude, no name filter
			result[crate_pkg] = None
		elif name:
			# only set a name filter if we haven't already seen a glob for this crate
			if crate_pkg not in result:
				result[crate_pkg] = set()
			if result[crate_pkg] is not None:
				result[crate_pkg].add(name)

	return result


def build_doc(crate_data: list[tuple[str, dict]]) -> str:
	"""assemble the single-file api reference from all surface crate data."""
	by_name = {name: data for name, data in crate_data}
	lines: list[str] = []

	# header from facade crate docs
	facade_data = by_name.get(FACADE_PACKAGE)
	name_filter: dict[str, set[str] | None] = {}
	if facade_data:
		name_filter = prelude_name_filter(facade_data)
		facade_items = facade_data["index"]
		facade_root_id = int(facade_data["root"])
		lines.append("# lunar engine api")
		# offset by 1 so '# quick start' → '## quick start'
		render_docs(lines, facade_items[str(facade_root_id)].get("docs"), heading_offset=1)

	# core api — one ### subsection per dep crate, filtered to prelude names
	lines.append("")
	lines.append("## core api")
	lines.append("")
	lines.append("available via `use lunar::prelude::*`.")
	for pkg_name, display_name in CORE_DEP_PACKAGES:
		data = by_name.get(pkg_name)
		if data is None:
			continue
		items = data["index"]
		root_id = int(data["root"])
		# None → glob import, no name filter; set → specific imports; missing → no filter
		allowed = name_filter.get(pkg_name, set() if pkg_name in name_filter else None)
		lines.append("")
		lines.append(f"### {display_name}")
		# offset by 3 so '# usage' → '#### usage' inside the ### subsection
		render_docs(lines, items[str(root_id)].get("docs"), heading_offset=3)
		render_surface_items(lines, items, root_id, 4, allowed_names=allowed)

	# plugins
	plugins = sorted(
		[(name, data) for name, data in crate_data if is_plugin_package(name)],
		key=lambda x: x[0],
	)
	if plugins:
		lines.append("")
		lines.append("## plugins")
		lines.append("")
		lines.append("add each with `app.add_plugin(XxxPlugin)`.")
		for pkg_name, data in plugins:
			items = data["index"]
			root_id = int(data["root"])
			short_name = pkg_name.removeprefix("lunar-plugin-").replace("-", "")
			lines.append("")
			lines.append(f"### {short_name} (`lunar::{short_name}`)")
			render_docs(lines, items[str(root_id)].get("docs"), heading_offset=3)
			render_surface_items(lines, items, root_id, 4)

	# other surface crates (pathfinding etc.)
	others = sorted(
		[
			(name, data) for name, data in crate_data
			if name != FACADE_PACKAGE and name not in CORE_DEP_NAMES and not is_plugin_package(name)
		],
		key=lambda x: x[0],
	)
	for pkg_name, data in others:
		items = data["index"]
		root_id = int(data["root"])
		short_name = pkg_name.removeprefix("lunar-").removesuffix("-rt").replace("-", "")
		lines.append("")
		lines.append(f"## {short_name} (`lunar::{short_name}`)")
		render_docs(lines, items[str(root_id)].get("docs"), heading_offset=2)
		render_surface_items(lines, items, root_id, 3)

	return "\n".join(lines).rstrip() + "\n"


def run_cargo_metadata(project_root: Path) -> dict:
	result = subprocess.run(
		["cargo", "metadata", "--format-version", "1", "--no-deps"],
		cwd=project_root,
		check=True,
		capture_output=True,
		text=True,
	)
	return json.loads(result.stdout)


def discover_json_files(target_dir: Path) -> list[Path]:
	doc_dir = target_dir / "doc"
	if not doc_dir.exists():
		return []
	return sorted(path for path in doc_dir.iterdir() if path.suffix == ".json")


def target_args_for_package(package: Mapping) -> list[list[str]]:
	lib_kinds = {"lib", "rlib", "dylib", "staticlib", "proc-macro"}
	lib_targets = []
	bin_targets = []
	for target in package.get("targets", []):
		kinds = set(target.get("kind", []))
		if kinds & lib_kinds:
			lib_targets.append(target["name"])
		elif "bin" in kinds:
			bin_targets.append(target["name"])
	if lib_targets:
		return [["--lib"]]
	return [["--bin", name] for name in bin_targets]


def latest_source_mtime(package_root: Path) -> float:
	latest = 0.0
	for path in package_root.rglob("*"):
		if path.is_dir() and path.name in {"target", ".git"}:
			continue
		if path.is_file() and (path.suffix == ".rs" or path.name in {"Cargo.toml", "build.rs"}):
			latest = max(latest, path.stat().st_mtime)
	return latest


def json_path_for_target(target_dir: Path, target_name: str) -> Path:
	return target_dir / "doc" / (target_name.replace("-", "_") + ".json")


def should_generate_json(package: Mapping, target_dir: Path, target_name: str, force: bool) -> bool:
	if force:
		return True
	json_path = json_path_for_target(target_dir, target_name)
	if not json_path.exists():
		return True
	package_root = Path(package["manifest_path"]).parent
	return json_path.stat().st_mtime < latest_source_mtime(package_root)


def generate_rustdoc_json(project_root: Path, metadata: dict, target_dir: Path, force: bool) -> None:
	workspace_members = set(metadata.get("workspace_members", []))
	packages = [pkg for pkg in metadata["packages"] if pkg["id"] in workspace_members]
	for pkg in packages:
		for target_args in target_args_for_package(pkg):
			target_name = pkg["name"] if target_args[0] != "--bin" else target_args[1]
			if not should_generate_json(pkg, target_dir, target_name, force):
				continue
			# only lunar-lib gates 2d/3d via cargo features
			extra = ["--features", "2d,3d"] if pkg["name"] == "lunar-lib" else []
			subprocess.run(
				[
					"cargo", "+nightly", "rustdoc",
					"-p", pkg["name"],
					*extra,
					*target_args,
					"--", "-Zunstable-options", "--output-format", "json",
				],
				cwd=project_root,
				check=True,
			)


def generate(project_root: Path, output_file: Path, force: bool) -> None:
	metadata = run_cargo_metadata(project_root)
	target_dir = Path(metadata["target_directory"])
	generate_rustdoc_json(project_root, metadata, target_dir, force)

	workspace_members = set(metadata.get("workspace_members", []))
	surface_by_stem = {
		pkg["name"].replace("-", "_"): pkg["name"]
		for pkg in metadata["packages"]
		if pkg["id"] in workspace_members and is_surface_package(pkg["name"])
	}

	crate_data: list[tuple[str, dict]] = []
	for json_path in discover_json_files(target_dir):
		pkg_name = surface_by_stem.get(json_path.stem)
		if pkg_name is not None:
			crate_data.append((pkg_name, load_json(json_path)))

	output_file.parent.mkdir(parents=True, exist_ok=True)
	output_file.write_text(build_doc(crate_data), encoding="utf-8")


def main() -> None:
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("project_root", type=Path, help="cargo project root")
	parser.add_argument("output_file", type=Path, help="output file path (e.g. llms.md)")
	parser.add_argument("--force", action="store_true", help="rebuild rustdoc json even if up-to-date")
	args = parser.parse_args()
	generate(args.project_root, args.output_file, args.force)
	print(args.output_file)


if __name__ == "__main__":
	main()

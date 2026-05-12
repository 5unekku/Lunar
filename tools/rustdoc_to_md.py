#!/usr/bin/env python
"""convert rustdoc json to markdown files organized like rustdoc html."""

from __future__ import annotations

import argparse
import json
import subprocess
from collections.abc import Iterable, Mapping
from pathlib import Path

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


def load_json(path: Path) -> dict:
	return json.loads(path.read_text(encoding="utf-8"))


def item_kind(item: Mapping) -> str:
	return next(iter(item["inner"].keys()))


def is_public(item: Mapping) -> bool:
	return item.get("visibility") == "public"


def doc_summary(docs: str | None) -> str | None:
	if not docs:
		return None
	lines = [line.strip() for line in docs.splitlines()]
	summary_lines = []
	for line in lines:
		if not line:
			if summary_lines:
				break
			continue
		summary_lines.append(line)
	if not summary_lines:
		return None
	return " ".join(summary_lines)


def filter_docs(docs: str) -> list[str]:
	lines = docs.rstrip().splitlines()
	kept: list[str] = []
	current_section = "summary"
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
		if stripped.startswith("#"):
			section_name = stripped.lstrip("#").strip().lower()
			current_section = section_name
			if section_name in allowed_sections:
				kept.append(line)
			continue
		if current_section not in allowed_sections:
			continue
		kept.append(line)
	return kept


def render_docs(lines: list[str], docs: str | None) -> None:
	if not docs:
		return
	filtered = filter_docs(docs)
	if not filtered:
		return
	lines.append("")
	lines.extend(filtered)


def reexport_targets(items: Mapping[str, Mapping], item_ids: Iterable[int]) -> list[Mapping]:
	targets = []
	for item_id in item_ids:
		item = items[str(item_id)]
		if not is_public(item):
			continue
		if "use" not in item["inner"]:
			continue
		use = item["inner"]["use"]
		target_id = use.get("id")
		if target_id is None:
			continue
		target = items.get(str(target_id))
		if not target:
			continue
		targets.append(target)
	return targets


def render_reexports(lines: list[str], items: Mapping[str, Mapping], item_ids: Iterable[int], heading_level: int) -> None:
	reexports = []
	for item_id in item_ids:
		item = items[str(item_id)]
		if not is_public(item):
			continue
		if "use" not in item["inner"]:
			continue
		use = item["inner"]["use"]
		name = use.get("name") or "_"
		source = use.get("source") or "_"
		summary = None
		target_id = use.get("id")
		if target_id is not None:
			target = items.get(str(target_id))
			if target:
				summary = doc_summary(target.get("docs"))
		entry = f"- {name} = {source}"
		if summary:
			entry = f"{entry} — {summary}"
		reexports.append(entry)
	if not reexports:
		return
	lines.append("")
	lines.append("#" * heading_level + " Re-exports")
	lines.extend(reexports)


def render_items_by_kind(
	lines: list[str],
	items: Mapping[str, Mapping],
	item_ids: Iterable[int],
	heading_level: int,
	extra_items: Iterable[Mapping] | None = None,
) -> None:
	by_kind: dict[str, list[Mapping]] = {}
	for item_id in item_ids:
		item = items[str(item_id)]
		if not is_public(item):
			continue
		kind = item_kind(item)
		if kind == "use":
			continue
		by_kind.setdefault(kind, []).append(item)

	if extra_items:
		for item in extra_items:
			kind = item_kind(item)
			if kind == "use":
				continue
			by_kind.setdefault(kind, []).append(item)

	for kind, title in KIND_TITLES.items():
		if kind not in by_kind:
			continue
		lines.append("")
		lines.append("#" * heading_level + f" {title}")
		for item in sorted(by_kind[kind], key=lambda i: i.get("name") or ""):
			name = item.get("name") or "(anonymous)"
			lines.append("")
			lines.append("#" * (heading_level + 1) + f" {name}")
			render_docs(lines, item.get("docs"))


def render_module_recursive(
	lines: list[str],
	items: Mapping[str, Mapping],
	module_id: int,
	heading_level: int,
	module_path: str,
	visited: set[int],
) -> None:
	if module_id in visited:
		return
	visited.add(module_id)

	module_item = items[str(module_id)]
	module_name = module_item.get("name") or "crate"
	full_path = f"{module_path}::{module_name}"
	lines.append("")
	lines.append("#" * heading_level + f" Module {full_path}")
	render_docs(lines, module_item.get("docs"))

	module_inner = module_item["inner"]["module"]
	child_ids = module_inner.get("items", [])

	reexported_items = reexport_targets(items, child_ids)
	render_reexports(lines, items, child_ids, heading_level + 1)
	render_items_by_kind(lines, items, child_ids, heading_level + 1, reexported_items)

	for item_id in child_ids:
		item = items[str(item_id)]
		if not is_public(item):
			continue
		if "module" not in item["inner"]:
			continue
		render_module_recursive(
			lines,
			items,
			item_id,
			heading_level + 1,
			full_path,
			visited,
		)


def convert(path: Path, output_root: Path) -> Path:
	data = load_json(path)
	items = data["index"]
	root_id = str(data["root"])
	root_item = items[root_id]
	crate_name = root_item.get("name") or path.stem

	lines: list[str] = []
	lines.append(f"# {crate_name}")
	render_docs(lines, root_item.get("docs"))

	root_module = root_item["inner"]["module"]
	root_children = root_module.get("items", [])

	reexported_items = reexport_targets(items, root_children)
	render_reexports(lines, items, root_children, 2)
	render_items_by_kind(lines, items, root_children, 2, reexported_items)

	visited: set[int] = set()
	for item_id in root_children:
		item = items[str(item_id)]
		if not is_public(item):
			continue
		if "module" not in item["inner"]:
			continue
		render_module_recursive(lines, items, item_id, 2, crate_name, visited)

	output_root.mkdir(parents=True, exist_ok=True)
	output_path = output_root / f"{crate_name}.md"
	output_path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")
	return output_path


def run_cargo_metadata(project_root: Path) -> dict:
	result = subprocess.run(
		["cargo", "metadata", "--format-version", "1", "--no-deps"],
		cwd=project_root,
		check=True,
		capture_output=True,
		text=True,
	)
	return json.loads(result.stdout)


def discover_json_files(project_root: Path, target_dir: Path) -> list[Path]:
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
	json_name = target_name.replace("-", "_") + ".json"
	return target_dir / "doc" / json_name


def should_generate_json(package: Mapping, target_dir: Path, target_name: str, force: bool) -> bool:
	if force:
		return True
	package_root = Path(package["manifest_path"]).parent
	json_path = json_path_for_target(target_dir, target_name)
	if not json_path.exists():
		return True
	json_mtime = json_path.stat().st_mtime
	return json_mtime < latest_source_mtime(package_root)


def generate_rustdoc_json(project_root: Path, metadata: dict, target_dir: Path, force: bool) -> None:
	workspace_members = set(metadata.get("workspace_members", []))
	packages = [pkg for pkg in metadata["packages"] if pkg["id"] in workspace_members]
	for pkg in packages:
		target_args_sets = target_args_for_package(pkg)
		if not target_args_sets:
			continue
		for target_args in target_args_sets:
			target_name = pkg["name"]
			if target_args and target_args[0] == "--bin":
				target_name = target_args[1]
			if not should_generate_json(pkg, target_dir, target_name, force):
				continue
			subprocess.run(
				[
					"cargo",
					"+nightly",
					"rustdoc",
					"-p",
					pkg["name"],
					*target_args,
					"--",
					"-Zunstable-options",
					"--output-format",
					"json",
				],
				cwd=project_root,
				check=True,
			)


def convert_project(project_root: Path, output_root: Path, force: bool) -> list[Path]:
	metadata = run_cargo_metadata(project_root)
	target_dir = Path(metadata["target_directory"])
	generate_rustdoc_json(project_root, metadata, target_dir, force)
	outputs = [convert(json_path, output_root) for json_path in discover_json_files(project_root, target_dir)]
	return outputs


def main() -> None:
	parser = argparse.ArgumentParser(description=__doc__)
	parser.add_argument("project_root", type=Path, help="cargo project root")
	parser.add_argument("output_root", type=Path, help="root directory for markdown docs")
	parser.add_argument("--force", action="store_true", help="rebuild rustdoc json even if up-to-date")
	args = parser.parse_args()

	outputs = convert_project(args.project_root, args.output_root, args.force)
	for output in outputs:
		print(output)


if __name__ == "__main__":
	main()

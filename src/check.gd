extends SceneTree

const RESULT_PREFIX := "GDKIT_CHECK_RESULT:"
const RESOURCE_KINDS := {
	"gd": "scripts",
	"tscn": "scenes",
	"scn": "scenes",
	"tres": "resources",
	"res": "resources",
	"gdshader": "resources",
}


func _initialize() -> void:
	var counts := { "scripts": 0, "scenes": 0, "resources": 0 }
	var failures: Array[String] = []
	_scan("res://", counts, failures)
	print(RESULT_PREFIX + JSON.stringify({ "counts": counts, "failures": failures }))
	quit(1 if not failures.is_empty() else 0)


func _scan(path: String, counts: Dictionary, failures: Array[String]) -> void:
	var directory := DirAccess.open(path)
	if directory == null:
		failures.append(path)
		return
	for child_directory in directory.get_directories():
		if child_directory.begins_with("."):
			continue
		var child_path := path.path_join(child_directory)
		if FileAccess.file_exists(child_path.path_join(".gdignore")):
			continue
		_scan(child_path, counts, failures)
	for file in directory.get_files():
		var kind: String = RESOURCE_KINDS.get(file.get_extension().to_lower(), "")
		if kind.is_empty():
			continue
		counts[kind] += 1
		var resource_path := path.path_join(file)
		if ResourceLoader.load(resource_path, "", ResourceLoader.CACHE_MODE_IGNORE) == null:
			failures.append(resource_path)

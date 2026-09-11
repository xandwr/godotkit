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
	var arguments := OS.get_cmdline_user_args()
	if arguments.size() != 1:
		push_error("Expected a resource manifest path")
		quit(2)
		return
	var paths = JSON.parse_string(FileAccess.get_file_as_string(arguments[0]))
	if not paths is Array:
		push_error("Invalid resource manifest")
		quit(2)
		return
	var counts := { "scripts": 0, "scenes": 0, "resources": 0 }
	var failures: Array[String] = []
	for resource_path in paths:
		var kind: String = RESOURCE_KINDS.get(resource_path.get_extension().to_lower(), "")
		if kind.is_empty():
			continue
		counts[kind] += 1
		if ResourceLoader.load(resource_path, "", ResourceLoader.CACHE_MODE_IGNORE) == null:
			failures.append(resource_path)
	print(RESULT_PREFIX + JSON.stringify({ "counts": counts, "failures": failures }))
	quit(1 if not failures.is_empty() else 0)

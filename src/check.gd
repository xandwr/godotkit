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
	paths.sort_custom(func(a: String, b: String) -> bool:
		var a_script := a.get_extension().to_lower() == "gd"
		var b_script := b.get_extension().to_lower() == "gd"
		return a_script if a_script != b_script else a < b
	)
	var counts := { "scripts": 0, "scenes": 0, "resources": 0 }
	var failures: Array[String] = []
	var loaded: Array[Resource] = []
	for resource_path in paths:
		var kind: String = RESOURCE_KINDS.get(resource_path.get_extension().to_lower(), "")
		if kind.is_empty():
			continue
		counts[kind] += 1
		var resource := ResourceLoader.load(resource_path)
		if resource == null:
			failures.append(resource_path)
		else:
			loaded.append(resource)
	loaded.clear()
	print(RESULT_PREFIX + JSON.stringify({ "counts": counts, "failures": failures }))
	quit(1 if not failures.is_empty() else 0)

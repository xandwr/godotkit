extends SceneTree

const PREFIX := "GDKIT_RESOURCE_RESULT:"


func fail(stage: String, message: String, field: String = "") -> void:
	print(PREFIX + JSON.stringify({ "status": "error", "stage": stage, "field": field, "message": message }))
	quit(1)


func _initialize() -> void:
	var arguments := OS.get_cmdline_user_args()
	var spec: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(arguments[0]))
	var resource: Resource
	if spec.has("class") and spec["class"] != null:
		var class_id := str(spec["class"])
		if not ClassDB.class_exists(class_id) or not ClassDB.can_instantiate(class_id) or not ClassDB.is_parent_class(class_id, "Resource"):
			fail("construct", "Class must be an instantiable Resource")
			return
		resource = ClassDB.instantiate(class_id)
	else:
		var script := load(str(spec.script)) as Script
		if script == null or not script.can_instantiate() or not ClassDB.is_parent_class(script.get_instance_base_type(), "Resource"):
			fail("construct", "Script must instantiate a Resource without constructor arguments")
			return
		var constructor_script := script
		while constructor_script != null:
			for method: Dictionary in constructor_script.get_script_method_list():
				if method.name == "_init" and method.args.size() > method.default_args.size():
					fail("construct", "Script constructor requires arguments")
					return
			constructor_script = constructor_script.get_base_script()
		resource = script.new() as Resource
	if resource == null:
		fail("construct", "Could not instantiate Resource")
		return
	var metadata := {}
	for property: Dictionary in resource.get_property_list():
		metadata[property.name] = property
	var expected := {}
	for field: String in spec.properties:
		if not metadata.has(field):
			fail("validate", "Unknown property", field)
			return
		var property: Dictionary = metadata[field]
		var usage := int(property.usage)
		if usage & PROPERTY_USAGE_STORAGE == 0 or usage & PROPERTY_USAGE_READ_ONLY != 0 or field == "script":
			fail("validate", "Property must be writable and serialized", field)
			return
		var value: Variant = spec.properties[field]
		var kind := int(property.type)
		if kind == TYPE_INT and value is float and is_finite(value) and value == floor(value) and abs(value) <= 9007199254740991.0:
			value = int(value)
		if kind not in [TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING] or typeof(value) != kind:
			fail("validate", "Expected %s; received %s" % [type_string(kind), type_string(typeof(value))], field)
			return
		expected[field] = value
	for field: String in expected:
		resource.set(field, expected[field])
	for field: String in expected:
		if resource.get(field) != expected[field]:
			fail("assign", "Setter changed requested value", field)
			return
	var error := ResourceSaver.save(resource, arguments[1])
	if error != OK:
		fail("save", error_string(error))
		return
	var reloaded := ResourceLoader.load(arguments[1], "", ResourceLoader.CACHE_MODE_IGNORE) as Resource
	if reloaded == null or reloaded.get_class() != resource.get_class() or reloaded.get_script() != resource.get_script():
		fail("verify", "Saved Resource did not reload with the requested type")
		return
	for field: String in expected:
		if typeof(reloaded.get(field)) != typeof(expected[field]) or reloaded.get(field) != expected[field]:
			fail("verify", "Saved value differs from requested value", field)
			return
	print(PREFIX + JSON.stringify({ "status": "created", "type": resource.get_class(), "script": spec.get("script"), "properties": expected }))
	quit()

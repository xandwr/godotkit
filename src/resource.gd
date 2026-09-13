extends SceneTree

const PREFIX := "GDKIT_RESOURCE_RESULT:"


func fail(stage: String, message: String, field: String = "") -> void:
	print(PREFIX + JSON.stringify({ "status": "error", "stage": stage, "field": field, "message": message }))
	quit(1)


func construct(spec: Dictionary) -> Resource:
	var resource: Resource
	if spec.has("class") and spec["class"] != null:
		var class_id := str(spec["class"])
		if not ClassDB.class_exists(class_id) or not ClassDB.can_instantiate(class_id) or not ClassDB.is_parent_class(class_id, "Resource"):
			fail("construct", "Class must be an instantiable Resource")
			return null
		resource = ClassDB.instantiate(class_id)
	else:
		var script := load(str(spec.script)) as Script
		if script == null or not script.can_instantiate() or not ClassDB.is_parent_class(script.get_instance_base_type(), "Resource"):
			fail("construct", "Script must instantiate a Resource without constructor arguments")
			return null
		var constructor_script := script
		while constructor_script != null:
			for method: Dictionary in constructor_script.get_script_method_list():
				if method.name == "_init" and method.args.size() > method.default_args.size():
					fail("construct", "Script constructor requires arguments")
					return null
			constructor_script = constructor_script.get_base_script()
		resource = script.new() as Resource
	if resource == null:
		fail("construct", "Could not instantiate Resource")
	return resource


func unsupported_reason(property: Dictionary) -> String:
	var usage := int(property.usage)
	if usage & PROPERTY_USAGE_STORAGE == 0 or usage & PROPERTY_USAGE_READ_ONLY != 0 or property.name == "script":
		return "Property must be writable and serialized"
	if int(property.type) not in [TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING]:
		return "Only boolean, integer, float, and string fields are supported"
	return ""


func property_metadata(resource: Resource) -> Dictionary:
	var metadata := {}
	for property: Dictionary in resource.get_property_list():
		if int(property.usage) & (PROPERTY_USAGE_GROUP | PROPERTY_USAGE_SUBGROUP | PROPERTY_USAGE_CATEGORY) == 0:
			metadata[property.name] = property
	return metadata


func default_value(value: Variant) -> Dictionary:
	var kind := typeof(value)
	if kind in [TYPE_NIL, TYPE_BOOL, TYPE_STRING] or (kind == TYPE_INT and value >= -9007199254740991 and value <= 9007199254740991) or (kind == TYPE_FLOAT and is_finite(value)):
		return { "encoding": "json", "value": value }
	if value is Resource:
		var script := value.get_script() as Script
		return { "encoding": "resource", "value": { "path": value.resource_path, "type": value.get_class(), "script": script.resource_path if script != null else null } }
	return { "encoding": "godot", "value": var_to_str(value) }


func enum_choices(property: Dictionary) -> Array:
	var choices := []
	if int(property.hint) not in [PROPERTY_HINT_ENUM, PROPERTY_HINT_ENUM_SUGGESTION]:
		return choices
	var enum_value := 0
	for option: String in str(property.hint_string).split(",", int(property.type) != TYPE_STRING):
		if int(property.type) == TYPE_STRING:
			choices.append({ "name": option, "value": option })
		elif int(property.type) == TYPE_INT:
			var parts := option.split(":")
			if parts.size() > 1:
				enum_value = parts[1].to_int()
			choices.append({ "name": parts[0], "value": enum_value })
			enum_value += 1
	return choices


func schema(resource: Resource, spec: Dictionary, metadata: Dictionary) -> void:
	var fields := []
	for field: String in metadata:
		var property: Dictionary = metadata[field]
		var reason := unsupported_reason(property)
		fields.append({
			"name": field,
			"type": type_string(int(property.type)),
			"type_id": int(property.type),
			"class_name": str(property.class_name),
			"default": default_value(resource.get(field)),
			"hint": int(property.hint),
			"hint_string": str(property.hint_string),
			"enum_choices": enum_choices(property),
			"usage": int(property.usage),
			"storage": int(property.usage) & PROPERTY_USAGE_STORAGE != 0,
			"read_only": int(property.usage) & PROPERTY_USAGE_READ_ONLY != 0,
			"editor_visible": int(property.usage) & PROPERTY_USAGE_EDITOR != 0,
			"create_supported": reason.is_empty(),
			"unsupported_reason": reason,
		})
	print(PREFIX + JSON.stringify({ "status": "schema", "type": resource.get_class(), "script": spec.get("script"), "executes_constructors_and_getters": true, "fields": fields, "integer_min": -9007199254740991, "integer_max": 9007199254740991 }))
	quit()


func _initialize() -> void:
	var arguments := OS.get_cmdline_user_args()
	var spec: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(arguments[0]))
	var resource := construct(spec)
	if resource == null: return
	var metadata := property_metadata(resource)
	if arguments[1] == "schema":
		schema(resource, spec, metadata)
		return
	var expected := {}
	for field: String in spec.properties:
		if not metadata.has(field):
			fail("validate", "Unknown property", field)
			return
		var property: Dictionary = metadata[field]
		var reason := unsupported_reason(property)
		if not reason.is_empty():
			fail("validate", reason, field)
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
	var error := ResourceSaver.save(resource, arguments[2])
	if error != OK:
		fail("save", error_string(error))
		return
	var reloaded := ResourceLoader.load(arguments[2], "", ResourceLoader.CACHE_MODE_IGNORE) as Resource
	if reloaded == null or reloaded.get_class() != resource.get_class() or reloaded.get_script() != resource.get_script():
		fail("verify", "Saved Resource did not reload with the requested type")
		return
	for field: String in expected:
		if typeof(reloaded.get(field)) != typeof(expected[field]) or reloaded.get(field) != expected[field]:
			fail("verify", "Saved value differs from requested value", field)
			return
	print(PREFIX + JSON.stringify({ "status": "created", "type": resource.get_class(), "script": spec.get("script"), "properties": expected }))
	quit()

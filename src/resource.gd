extends SceneTree

const PREFIX := "GDKIT_RESOURCE_RESULT:"
const MAX_DEPTH := 16

var failed := false


func fail(stage: String, message: String, field: String = "") -> void:
	if failed: return
	failed = true
	print(PREFIX + JSON.stringify({ "status": "error", "stage": stage, "field": field, "message": message }))
	quit(1)


func construct(spec: Dictionary, field: String = "") -> Resource:
	var resource: Resource
	if spec.has("class") and spec["class"] != null:
		var class_id := str(spec["class"])
		if not ClassDB.class_exists(class_id) or not ClassDB.can_instantiate(class_id) or not ClassDB.is_parent_class(class_id, "Resource"):
			fail("construct", "Class must be an instantiable Resource", field)
			return null
		resource = ClassDB.instantiate(class_id)
	else:
		var script := load(str(spec.script)) as Script
		if script == null or not script.can_instantiate() or not ClassDB.is_parent_class(script.get_instance_base_type(), "Resource"):
			fail("construct", "Script must instantiate a Resource without constructor arguments", field)
			return null
		var constructor_script := script
		while constructor_script != null:
			for method: Dictionary in constructor_script.get_script_method_list():
				if method.name == "_init" and method.args.size() > method.default_args.size():
					fail("construct", "Script constructor requires arguments", field)
					return null
			constructor_script = constructor_script.get_base_script()
		resource = script.new() as Resource
	if resource == null:
		fail("construct", "Could not instantiate Resource", field)
	return resource


func unsupported_reason(property: Dictionary) -> String:
	var usage := int(property.usage)
	if usage & PROPERTY_USAGE_STORAGE == 0 or usage & PROPERTY_USAGE_READ_ONLY != 0 or property.name == "script":
		return "Property must be writable and serialized"
	if int(property.type) == TYPE_OBJECT:
		if resource_constraints(property).is_empty():
			return "Object field must declare a Resource type"
	elif int(property.type) == TYPE_ARRAY:
		if not property.has("element") or resource_constraints(property.element).is_empty():
			return "Array must declare a Resource element type"
	elif int(property.type) not in [TYPE_BOOL, TYPE_INT, TYPE_FLOAT, TYPE_STRING]:
		return "Only scalar and Resource fields are supported"
	return ""


func resource_constraints(property: Dictionary) -> Array:
	var constraints := []
	if int(property.type) != TYPE_OBJECT: return constraints
	if property.has("element_script"):
		var element_script: Script = property.element_script
		if ClassDB.is_parent_class(element_script.get_instance_base_type(), "Resource"):
			constraints.append({ "class": element_script.get_instance_base_type(), "script": element_script.resource_path })
		return constraints
	var names := str(property.hint_string) if int(property.hint) == PROPERTY_HINT_RESOURCE_TYPE else str(property.class_name)
	for class_id: String in names.split(",", false):
		class_id = class_id.strip_edges()
		if ClassDB.class_exists(class_id) and ClassDB.is_parent_class(class_id, "Resource"):
			constraints.append({ "class": class_id, "script": null })
		else:
			for entry: Dictionary in ProjectSettings.get_global_class_list():
				if entry.class == class_id:
					var script := load(str(entry.path)) as Script
					if script != null and ClassDB.is_parent_class(script.get_instance_base_type(), "Resource"):
						constraints.append({ "class": class_id, "script": str(entry.path) })
	return constraints


func accepts_resource(property: Dictionary, value: Resource) -> bool:
	if value == null: return true
	for constraint: Dictionary in resource_constraints(property):
		if constraint.script != null:
			if is_instance_of(value, load(str(constraint.script))): return true
		elif value.is_class(str(constraint.class)):
			return true
	return false


func property_metadata(resource: Resource) -> Dictionary:
	var metadata := {}
	for property: Dictionary in resource.get_property_list():
		if int(property.usage) & (PROPERTY_USAGE_GROUP | PROPERTY_USAGE_SUBGROUP | PROPERTY_USAGE_CATEGORY) == 0:
			if int(property.type) == TYPE_ARRAY:
				var current: Variant = resource.get(property.name)
				if current is Array and current.get_typed_builtin() == TYPE_OBJECT:
					var element_script := current.get_typed_script() as Script
					property["element"] = { "type": TYPE_OBJECT, "hint": PROPERTY_HINT_NONE, "hint_string": "", "class_name": current.get_typed_class_name() }
					if element_script != null:
						property.element["element_script"] = element_script
			metadata[property.name] = property
	return metadata


func default_value(value: Variant) -> Dictionary:
	var kind := typeof(value)
	if kind in [TYPE_NIL, TYPE_OBJECT] and value == null:
		return { "encoding": "json", "value": null }
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
			"resource_constraints": resource_constraints(property),
			"element_constraints": resource_constraints(property.element) if property.has("element") else [],
			"element_accepted_inputs": ["null", "$ref", "$resource"] if reason.is_empty() and int(property.type) == TYPE_ARRAY else [],
			"accepted_inputs":["null", "$ref", "$resource"] if reason.is_empty() and int(property.type) == TYPE_OBJECT else(["array"] if reason.is_empty() and int(property.type) == TYPE_ARRAY else (["scalar"] if reason.is_empty() else[])),
		})
	print(PREFIX + JSON.stringify({ "status": "schema", "type": resource.get_class(), "script": spec.get("script"), "executes_constructors_and_getters": true, "fields": fields, "integer_min": -9007199254740991, "integer_max": 9007199254740991, "max_resource_depth": MAX_DEPTH }))
	quit()


func field_path(prefix: String, field: String) -> String:
	return field if prefix.is_empty() else prefix + ".properties." + field


func snapshot(value: Variant, path: String, ancestors: Array = [], depth: int = 0) -> Variant:
	if typeof(value) in [TYPE_NIL, TYPE_OBJECT] and value == null:
		return { "kind": "null" }
	if depth > MAX_DEPTH and (value is Resource or value is Array or value is Dictionary):
		fail("validate", "Serialized graph nesting exceeds limit of 16", path)
		return null
	if value is Script:
		return { "kind": "script", "path": value.resource_path }
	if value is Resource:
		var identity: int = value.get_instance_id()
		if identity in ancestors:
			fail("validate", "Cyclic Resource graph is unsupported", path)
			return null
		var chain := ancestors.duplicate()
		chain.append(identity)
		var fields := {}
		for property: Dictionary in value.get_property_list():
			if int(property.usage) & PROPERTY_USAGE_STORAGE != 0 and property.name != "script":
				fields[property.name] = snapshot(value.get(property.name), field_path(path, property.name), chain, depth + 1)
				if failed: return null
		var script := value.get_script() as Script
		return { "kind": "resource", "type": value.get_class(), "script": script.resource_path if script != null else null, "properties": fields }
	if value is Array:
		var items := []
		for index in value.size():
			items.append(snapshot(value[index], path + "[%d]" % index, ancestors, depth + 1))
			if failed: return null
		var typed_script := value.get_typed_script() as Script
		return { "kind": "array", "builtin": value.get_typed_builtin(), "class": str(value.get_typed_class_name()), "script": typed_script.resource_path if typed_script != null else null, "items": items }
	if value is Dictionary:
		var items := {}
		for key: Variant in value:
			if typeof(key) in [TYPE_OBJECT, TYPE_ARRAY, TYPE_DICTIONARY]:
				fail("validate", "Compound serialized dictionary keys are unsupported", path)
				return null
			items[var_to_str(key)] = snapshot(value[key], path + "[%s]" % var_to_str(key), ancestors, depth + 1)
			if failed: return null
		return { "kind": "dictionary", "items": items }
	if typeof(value) == TYPE_OBJECT:
		fail("validate", "Serialized non-Resource objects are unsupported", path)
		return null
	return { "kind": typeof(value), "value": value }


func compare_snapshot(actual: Variant, expected: Variant, path: String, stage: String) -> bool:
	if actual == expected: return true
	if actual is Dictionary and expected is Dictionary and str(actual.get("kind")) == str(expected.get("kind")):
		var key := "properties" if str(expected.get("kind")) == "resource" else "items"
		if expected.has(key) and actual.has(key):
			var left: Variant = actual[key]
			var right: Variant = expected[key]
			if left is Dictionary and right is Dictionary and left.size() == right.size():
				for field: String in right:
					if left.has(field) and not compare_snapshot(left[field], right[field], field_path(path, field), stage): return false
			elif left is Array and right is Array and left.size() == right.size():
				for index in right.size():
					if not compare_snapshot(left[index], right[index], path + "[%d]" % index, stage): return false
	fail(stage, "Serialized value differs from requested graph", path)
	return false


func check_requested(resource: Resource, expected: Dictionary, path: String, stage: String, identity: bool) -> bool:
	for field: String in expected:
		var descriptor: Dictionary = expected[field]
		var actual: Variant = resource.get(field)
		var location := field_path(path, field)
		if not check_value(actual, descriptor, "properties." + location if path.is_empty() and descriptor.kind != "scalar" else location, stage, identity): return false
	return true


func check_value(actual: Variant, descriptor: Dictionary, location: String, stage: String, identity: bool) -> bool:
	if descriptor.kind == "array":
		if not actual is Array or actual.size() != descriptor.entries.size():
			fail(stage, "Array assignment changed requested entries", location)
			return false
		for index in descriptor.entries.size():
			if not check_value(actual[index], descriptor.entries[index], location + "[%d]" % index, stage, identity): return false
		return compare_snapshot(snapshot(actual, location), descriptor.snapshot, location, stage)
	if descriptor.kind == "scalar":
		if descriptor.value == null and actual == null: return true
		if typeof(actual) != typeof(descriptor.value) or actual != descriptor.value:
			fail(stage, "Value differs from requested value", location)
			return false
	else:
		if not actual is Resource or (identity and actual != descriptor.value):
			fail(stage, "Resource assignment changed requested object or rejected its script type", location)
			return false
		if descriptor.kind == "ref" and actual.resource_path != descriptor.path:
			fail(stage, "External Resource reference changed path", location)
			return false
		var observed: Variant = snapshot(actual, location)
		if failed or not compare_snapshot(observed, descriptor.snapshot, location, stage): return false
		if descriptor.kind == "inline" and not check_requested(actual, descriptor.expected, location, stage, identity): return false
	return true


func resource_input(value: Variant, property: Dictionary, location: String, depth: int, error_location: String = "") -> Dictionary:
	var descriptor := { "kind": "scalar", "value": value }
	if value == null: return descriptor
	if error_location.is_empty(): error_location = location
	if not value is Dictionary:
		fail("validate", "Expected null, $ref, or $resource", error_location)
		return {}
	var nested_path := location
	if value.has("$ref"):
		var reference := ResourceLoader.load(str(value["$ref"]), "", ResourceLoader.CACHE_MODE_IGNORE_DEEP)
		if reference == null:
			fail("load", "Could not load referenced Resource", error_location)
			return {}
		descriptor = { "kind": "ref", "value": reference, "path": reference.resource_path, "snapshot": snapshot(reference, nested_path) }
	else:
		var child := build(value["$resource"], nested_path, depth + 1)
		if failed: return {}
		descriptor = { "kind": "inline", "value": child.resource, "expected": child.expected, "snapshot": snapshot(child.resource, nested_path) }
	if failed: return {}
	value = descriptor.value
	if not accepts_resource(property, value):
		fail("validate", "Resource does not satisfy declared class or script constraints: %s" % resource_constraints(property), error_location)
		return {}
	return descriptor


func build(spec: Dictionary, path: String = "", depth: int = 0) -> Dictionary:
	if depth > MAX_DEPTH:
		fail("validate", "Resource nesting exceeds limit of 16", path)
		return {}
	var resource := construct(spec, path)
	if failed or resource == null: return {}
	var metadata := property_metadata(resource)
	var expected := {}
	for field: String in spec.properties:
		var location := field_path(path, field)
		if not metadata.has(field):
			fail("validate", "Unknown property", location)
			return {}
		var property: Dictionary = metadata[field]
		var reason := unsupported_reason(property)
		if not reason.is_empty():
			fail("validate", reason, location)
			return {}
		var value: Variant = spec.properties[field]
		var kind := int(property.type)
		var descriptor := { "kind": "scalar", "value": value }
		if kind == TYPE_ARRAY:
			if not value is Array:
				fail("validate", "Expected an array of Resources", location)
				return {}
			var entries := []
			var array_path := "properties." + location if path.is_empty() else location
			var template: Array = resource.get(field)
			var typed := Array([], template.get_typed_builtin(), template.get_typed_class_name(), template.get_typed_script())
			for index in value.size():
				var entry := resource_input(value[index], property.element, array_path + "[%d]" % index, depth)
				if failed: return {}
				entries.append(entry)
				typed.append(entry.value)
			descriptor = { "kind": "array", "value": typed, "entries": entries, "snapshot": snapshot(typed, array_path) }
		elif kind == TYPE_OBJECT and value != null:
			descriptor = resource_input(value, property, "properties." + location if path.is_empty() else location, depth, location)
			if failed: return {}
			value = descriptor.value
		elif kind != TYPE_OBJECT:
			if kind == TYPE_INT and value is float and is_finite(value) and value == floor(value) and abs(value) <= 9007199254740991.0:
				value = int(value)
			if typeof(value) != kind:
				fail("validate", "Expected %s; received %s" % [type_string(kind), type_string(typeof(value))], location)
				return {}
			descriptor.value = value
		expected[field] = descriptor
	for field: String in expected:
		resource.set(field, expected[field].value)
		if expected[field].kind == "scalar": spec.properties[field] = expected[field].value
	if not check_requested(resource, expected, path, "assign", true): return {}
	return { "resource": resource, "expected": expected }


func _initialize() -> void:
	var arguments := OS.get_cmdline_user_args()
	var spec: Dictionary = JSON.parse_string(FileAccess.get_file_as_string(arguments[0]))
	if arguments[1] == "schema":
		var instance := construct(spec)
		if failed or instance == null: return
		schema(instance, spec, property_metadata(instance))
		return
	var graph := build(spec)
	if failed: return
	var resource: Resource = graph.resource
	var baseline: Variant = snapshot(resource, "")
	if failed: return
	var error := ResourceSaver.save(resource, arguments[2])
	if error != OK:
		fail("save", error_string(error))
		return
	var saved_observation: Variant = snapshot(resource, "")
	if failed or not compare_snapshot(saved_observation, baseline, "", "save"): return
	var reloaded := ResourceLoader.load(arguments[2], "", ResourceLoader.CACHE_MODE_IGNORE_DEEP) as Resource
	if reloaded == null:
		fail("verify", "Saved Resource did not reload")
		return
	if not check_requested(reloaded, graph.expected, "", "verify", false): return
	var reloaded_observation: Variant = snapshot(reloaded, "")
	if failed or not compare_snapshot(reloaded_observation, baseline, "", "verify"): return
	print(PREFIX + JSON.stringify({ "status": "created", "type": resource.get_class(), "script": spec.get("script"), "properties": spec.properties }))
	quit()

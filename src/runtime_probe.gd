extends SceneTree

const SCHEMA_VERSION := 1
const MAX_REQUEST_BYTES := 16384
const MAX_EVENTS := 128
const MAX_NODES := 4096

var server := TCPServer.new()
var peer: StreamPeerTCP
var buffer := PackedByteArray()
var generation := ""
var token := ""
var physics_tick := 0
var observed_apis := {}
var events: Array[Dictionary] = []


func _initialize() -> void:
	generation = OS.get_environment("GDKIT_PROBE_GENERATION")
	token = OS.get_environment("GDKIT_PROBE_TOKEN")
	var ready_path := OS.get_environment("GDKIT_PROBE_READY")
	if generation.is_empty() or token.is_empty() or ready_path.is_empty():
		quit(2)
		return
	if server.listen(0, "127.0.0.1") != OK:
		quit(2)
		return
	var ready := FileAccess.open(ready_path, FileAccess.WRITE)
	if ready == null:
		quit(2)
		return
	ready.store_string(JSON.stringify({
		"schema_version": SCHEMA_VERSION,
		"generation": generation,
		"port": server.get_local_port(),
	}))
	ready.close()
	call_deferred("launch_scene")


func launch_scene() -> void:
	var scene := OS.get_environment("GDKIT_PROBE_SCENE")
	if scene.is_empty():
		scene = ProjectSettings.get_setting("application/run/main_scene", "")
	if scene.is_empty() or change_scene_to_file(scene) != OK:
		quit(1)


func _physics_process(_delta: float) -> bool:
	physics_tick += 1
	return false


func _process(_delta: float) -> bool:
	discover_multiplayer_apis()
	poll_socket()
	return false


func poll_socket() -> void:
	if peer == null and server.is_connection_available():
		peer = server.take_connection()
		peer.set_no_delay(true)
		buffer.clear()
	if peer == null:
		return
	peer.poll()
	if peer.get_status() != StreamPeerTCP.STATUS_CONNECTED:
		peer = null
		return
	var available := peer.get_available_bytes()
	if available == 0:
		return
	var data := peer.get_data(available)
	if data[0] != OK:
		peer.disconnect_from_host()
		peer = null
		return
	buffer.append_array(data[1])
	if buffer.size() > MAX_REQUEST_BYTES:
		peer.disconnect_from_host()
		peer = null
		return
	if buffer.has(10):
		var request = JSON.parse_string(buffer.get_string_from_utf8())
		if not valid_request(request):
			peer.disconnect_from_host()
			peer = null
			return
		call_deferred("answer", request)


func valid_request(request: Variant) -> bool:
	if not request is Dictionary or request.size() != 7:
		return false
	return request.get("schema_version") == SCHEMA_VERSION \
		and request.get("request_id") is String \
		and not request.request_id.is_empty() \
		and request.get("generation") == generation \
		and request.get("token") == token \
		and request.get("kind") == "network_observation" \
		and request.get("deadline_unix_ms") is float \
		and request.deadline_unix_ms >= Time.get_unix_time_from_system() * 1000.0 \
		and request.get("max_response_bytes") is float \
		and request.max_response_bytes >= 4096.0 \
		and request.max_response_bytes <= 8388608.0


func answer(request: Dictionary) -> void:
	if request.deadline_unix_ms < Time.get_unix_time_from_system() * 1000.0:
		peer.disconnect_from_host()
		peer = null
		buffer.clear()
		return
	var observation := collect_observation()
	var response := {
		"schema_version": SCHEMA_VERSION,
		"request_id": request.request_id,
		"generation": generation,
		"observation": observation,
	}
	var encoded := (JSON.stringify(response) + "\n").to_utf8_buffer()
	while encoded.size() > int(request.max_response_bytes) and not observation.node_authorities.is_empty():
		observation.truncated = true
		observation.node_authorities.resize(observation.node_authorities.size() / 2)
		encoded = (JSON.stringify(response) + "\n").to_utf8_buffer()
	while encoded.size() > int(request.max_response_bytes) and not observation.recent_events.is_empty():
		observation.truncated = true
		observation.recent_events.resize(observation.recent_events.size() / 2)
		encoded = (JSON.stringify(response) + "\n").to_utf8_buffer()
	while encoded.size() > int(request.max_response_bytes) and not observation.synchronizers.is_empty():
		observation.truncated = true
		observation.synchronizers.resize(observation.synchronizers.size() / 2)
		encoded = (JSON.stringify(response) + "\n").to_utf8_buffer()
	while encoded.size() > int(request.max_response_bytes) and not observation.spawners.is_empty():
		observation.truncated = true
		observation.spawners.resize(observation.spawners.size() / 2)
		encoded = (JSON.stringify(response) + "\n").to_utf8_buffer()
	while encoded.size() > int(request.max_response_bytes) and observation.multiplayer_roots.size() > 1:
		observation.truncated = true
		observation.multiplayer_roots.resize(observation.multiplayer_roots.size() / 2)
		encoded = (JSON.stringify(response) + "\n").to_utf8_buffer()
	if request.deadline_unix_ms >= Time.get_unix_time_from_system() * 1000.0 \
		and encoded.size() <= int(request.max_response_bytes):
		peer.put_data(encoded)
	peer.disconnect_from_host()
	peer = null
	buffer.clear()


func collect_observation() -> Dictionary:
	var roots: Array[Dictionary] = []
	var authorities: Array[Dictionary] = []
	var spawners: Array[Dictionary] = []
	var synchronizers: Array[Dictionary] = []
	var nodes: Array[Node] = []
	collect_nodes(root, nodes)
	var root_apis := {}
	for node in nodes:
		var api := node.get_multiplayer()
		var api_id := api.get_instance_id()
		var parent := node.get_parent()
		if parent == null or parent.get_multiplayer().get_instance_id() != api_id:
			root_apis[node.get_path()] = api
		if authorities.size() < MAX_NODES:
			authorities.append({
				"path": str(node.get_path()),
				"authority": node.get_multiplayer_authority(),
				"local_authority": node.is_multiplayer_authority(),
			})
		if node is MultiplayerSpawner:
			spawners.append(spawner_state(node))
		elif node is MultiplayerSynchronizer:
			synchronizers.append(synchronizer_state(node))
	for path in root_apis:
		roots.append(multiplayer_state(str(path), root_apis[path]))
	return {
		"collected_at_unix_ms": int(Time.get_unix_time_from_system() * 1000.0),
		"process_tick": get_frame(),
		"physics_tick": physics_tick,
		"multiplayer_roots": roots,
		"node_authorities": authorities,
		"spawners": spawners,
		"synchronizers": synchronizers,
		"recent_events": events.duplicate(true),
		"truncated": nodes.size() > authorities.size(),
	}


func collect_nodes(node: Node, nodes: Array[Node]) -> void:
	nodes.append(node)
	for child in node.get_children(true):
		collect_nodes(child, nodes)


func multiplayer_state(path: String, api: MultiplayerAPI) -> Dictionary:
	var peer_class := "<none>"
	var status := "disconnected"
	if api.has_multiplayer_peer():
		var multiplayer_peer := api.get_multiplayer_peer()
		peer_class = multiplayer_peer.get_class()
		status = ["disconnected", "connecting", "connected"][multiplayer_peer.get_connection_status()]
	var authenticating: Array[int] = []
	var configuration := {
		"root_path": "",
		"refusing_new_connections": null,
		"object_decoding_allowed": null,
		"server_relay_enabled": null,
		"auth_timeout": null,
		"max_sync_packet_size": null,
		"max_delta_packet_size": null,
	}
	if api is SceneMultiplayer:
		authenticating.assign(api.get_authenticating_peers())
		configuration = {
			"root_path": str(api.root_path),
			"refusing_new_connections": api.refuse_new_connections,
			"object_decoding_allowed": api.allow_object_decoding,
			"server_relay_enabled": api.server_relay,
			"auth_timeout": api.auth_timeout,
			"max_sync_packet_size": api.max_sync_packet_size,
			"max_delta_packet_size": api.max_delta_packet_size,
		}
	var connected: Array[int] = []
	connected.assign(api.get_peers())
	return {
		"root": path,
		"api_class": api.get_class(),
		"peer_class": peer_class,
		"connection_status": status,
		"local_peer_id": api.get_unique_id(),
		"is_server": api.is_server(),
		"connected_peers": connected,
		"authenticating_peers": authenticating,
		"configuration": configuration,
	}


func spawner_state(spawner: MultiplayerSpawner) -> Dictionary:
	var scenes: Array[String] = []
	for index in spawner.get_spawnable_scene_count():
		scenes.append(spawner.get_spawnable_scene(index))
	var spawn_root := spawner.get_node_or_null(spawner.spawn_path)
	return {
		"path": str(spawner.get_path()),
		"authority": spawner.get_multiplayer_authority(),
		"spawn_path": str(spawner.spawn_path),
		"spawn_limit": spawner.spawn_limit,
		"spawn_path_child_count": spawn_root.get_child_count() if spawn_root != null else 0,
		"spawnable_scenes": scenes,
	}


func synchronizer_state(synchronizer: MultiplayerSynchronizer) -> Dictionary:
	var properties: Array[Dictionary] = []
	var config := synchronizer.replication_config
	if config != null:
		for path in config.get_properties():
			properties.append({
				"path": str(path),
				"spawn": config.property_get_spawn(path),
				"sync": config.property_get_sync(path),
				"watch": config.property_get_watch(path),
				"mode": config.property_get_replication_mode(path),
			})
	return {
		"path": str(synchronizer.get_path()),
		"authority": synchronizer.get_multiplayer_authority(),
		"root_path": str(synchronizer.root_path),
		"replication_interval": synchronizer.replication_interval,
		"delta_interval": synchronizer.delta_interval,
		"visibility_update_mode": synchronizer.visibility_update_mode,
		"public_visibility": synchronizer.public_visibility,
		"properties": properties,
	}


func discover_multiplayer_apis() -> void:
	var nodes: Array[Node] = []
	collect_nodes(root, nodes)
	for node in nodes:
		var api := node.get_multiplayer()
		var api_id := api.get_instance_id()
		if observed_apis.has(api_id):
			continue
		observed_apis[api_id] = api
		api.peer_connected.connect(capture_peer_event.bind(api_id, "peer_connected"))
		api.peer_disconnected.connect(capture_peer_event.bind(api_id, "peer_disconnected"))
		api.connected_to_server.connect(capture_api_event.bind(api_id, "connected_to_server"))
		api.connection_failed.connect(capture_api_event.bind(api_id, "connection_failed"))
		api.server_disconnected.connect(capture_api_event.bind(api_id, "server_disconnected"))
		if api is SceneMultiplayer:
			api.peer_authenticating.connect(capture_peer_event.bind(api_id, "peer_authenticating"))
			api.peer_authentication_failed.connect(capture_peer_event.bind(api_id, "peer_authentication_failed"))


func capture_peer_event(peer_id: int, api_id: int, kind: String) -> void:
	append_event({
		"kind": kind,
		"api_instance_id": api_id,
		"peer_id": peer_id,
		"direction": null,
		"node_path": null,
		"bytes": null,
		"count": null,
	})


func capture_api_event(api_id: int, kind: String) -> void:
	append_event({
		"kind": kind,
		"api_instance_id": api_id,
		"peer_id": null,
		"direction": null,
		"node_path": null,
		"bytes": null,
		"count": null,
	})


func append_event(event: Dictionary) -> void:
	event.collected_at_unix_ms = int(Time.get_unix_time_from_system() * 1000.0)
	event.process_tick = get_frame()
	events.append(event)
	if events.size() > MAX_EVENTS:
		events.pop_front()

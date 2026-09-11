@tool
extends SceneTree

class Capture extends Logger:
	var mutex := Mutex.new()
	var lines: Array[String] = []


	func _log_error(function: String, file: String, line: int, code: String, rationale: String, _notify: bool, kind: int, backtraces: Array[ScriptBacktrace]) -> void:
		mutex.lock()
		var prefix := "WARNING" if kind == Logger.ERROR_TYPE_WARNING else "ERROR"
		if kind == Logger.ERROR_TYPE_SCRIPT:
			prefix = "SCRIPT ERROR"
		lines.append("%s: %s" % [prefix, rationale if not rationale.is_empty() else code])
		lines.append("   at: %s (%s:%d)" % [function, file, line])
		for trace in backtraces:
			lines.append(trace.format())
		mutex.unlock()


	func take() -> String:
		mutex.lock()
		var text := "\n".join(lines)
		lines.clear()
		mutex.unlock()
		return text


	func _log_message(message: String, error: bool) -> void:
		if error:
			mutex.lock()
			lines.append(message)
			mutex.unlock()

var capture := Capture.new()
var server := TCPServer.new()
var peer: StreamPeerTCP
var buffer := PackedByteArray()
var token := ""
var startup := ""
var busy := true
var last_activity := Time.get_ticks_msec()


func _init() -> void:
	OS.add_logger(capture)


func _initialize() -> void:
	call_deferred("start")


func start() -> void:
	var arguments := OS.get_cmdline_user_args()
	if arguments.size() != 2:
		quit(2)
		return
	token = arguments[1]
	var filesystem := EditorInterface.get_resource_filesystem()
	await process_frame
	while filesystem.is_scanning() or filesystem.is_importing():
		await process_frame
	startup = capture.take()
	if server.listen(0, "127.0.0.1") != OK:
		quit(2)
		return
	var ready := FileAccess.open(arguments[0], FileAccess.WRITE)
	if ready == null:
		quit(2)
		return
	ready.store_string(JSON.stringify({ "port": server.get_local_port(), "pid": OS.get_process_id() }))
	ready.close()
	busy = false


func _process(_delta: float) -> bool:
	if busy:
		return false
	if Time.get_ticks_msec() - last_activity > 300000:
		quit()
		return false
	if peer == null and server.is_connection_available():
		peer = server.take_connection()
		peer.set_no_delay(true)
		last_activity = Time.get_ticks_msec()
		buffer.clear()
	if peer == null:
		return false
	peer.poll()
	if peer.get_status() != StreamPeerTCP.STATUS_CONNECTED or Time.get_ticks_msec() - last_activity > 5000:
		peer.disconnect_from_host()
		peer = null
		return false
	var available := peer.get_available_bytes()
	if available > 0:
		var data := peer.get_data(available)
		if data[0] != OK:
			peer.disconnect_from_host()
			peer = null
			return false
		buffer.append_array(data[1])
		if buffer.size() > 8388608:
			peer.disconnect_from_host()
			peer = null
			return false
		if buffer.has(10):
			var request = JSON.parse_string(buffer.get_string_from_utf8())
			if not request is Dictionary or request.get("token") != token:
				peer.disconnect_from_host()
				peer = null
				return false
			busy = true
			call_deferred("check", request)
	return false


func check(request: Dictionary) -> void:
	if request.get("stop", false):
		peer.put_data("{}\n".to_utf8_buffer())
		quit()
		return
	var filesystem := EditorInterface.get_resource_filesystem()
	while filesystem.is_scanning() or filesystem.is_importing():
		await process_frame
	for path in request.get("changed_scripts", []):
		if path is String and path.begins_with("res://") and path.ends_with(".gd"):
			filesystem.update_file(path)
	filesystem.scan()
	await process_frame
	while filesystem.is_scanning() or filesystem.is_importing():
		await process_frame
	var messages := startup + "\n" + capture.take()
	peer.put_data((JSON.stringify({ "diagnostics": messages }) + "\n").to_utf8_buffer())
	peer.disconnect_from_host()
	peer = null
	last_activity = Time.get_ticks_msec()
	busy = false

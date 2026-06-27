mergeInto(LibraryManager.library, {
	js_read_file: (file_id, offset, buffer_ptr, length) => {
		const dataBuffer = Module.js_read_file(file_id, offset, length);
		if (!dataBuffer) return -1;

		const u8 = new Uint8Array(dataBuffer);
		HEAPU8.set(u8, buffer_ptr);

		return u8.length;
	},
	js_get_file_size: (file_id) => Module.js_get_file_size(file_id),
});

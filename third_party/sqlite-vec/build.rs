fn main() {
    cc::Build::new()
        .file("sqlite-vec.c")
        .define("SQLITE_CORE", None)
        // Upstream C emits -Wunused-parameter warnings (p_vector in
        // sqlite-vec-diskann.c, pApi in sqlite-vec.c). Silence only that
        // category so the vendored C files stay byte-identical to upstream.
        .flag_if_supported("-Wno-unused-parameter")
        .compile("sqlite_vec0");
}

// Compile this harness with SQLITE_CORE and link libsqlite3 + libm.
#ifndef GROW_SQLITE_VEC_SOURCE_PATH
#define GROW_SQLITE_VEC_SOURCE_PATH "../sqlite-vec.c"
#endif
#include GROW_SQLITE_VEC_SOURCE_PATH
#include <stdio.h>

static vec0_vtab *captured;
static int capture_create(sqlite3 *db, void *aux, int argc,
                          const char *const *argv, sqlite3_vtab **out, char **err) {
  int rc = vec0Create(db, aux, argc, argv, out, err);
  if (rc == SQLITE_OK) captured = (vec0_vtab *)*out;
  return rc;
}

static void execute(sqlite3 *db, const char *sql) {
  char *err = NULL;
  int rc = sqlite3_exec(db, sql, NULL, NULL, &err);
  if (rc != SQLITE_OK) fprintf(stderr, "%s: %s\n", sql, err);
  sqlite3_free(err);
  assert(rc == SQLITE_OK);
}

static void nearest(sqlite3 *db, const char *sql, int expected) {
  sqlite3_stmt *stmt = NULL;
  assert(sqlite3_prepare_v2(db, sql, -1, &stmt, NULL) == SQLITE_OK);
  assert(sqlite3_step(stmt) == SQLITE_ROW);
  assert(sqlite3_column_int(stmt, 0) == expected);
  assert(sqlite3_step(stmt) == SQLITE_DONE);
  assert(sqlite3_finalize(stmt) == SQLITE_OK);
}

int main(int argc, char **argv) {
  sqlite3 *db = NULL;
  assert(sqlite3_open(":memory:", &db) == SQLITE_OK);
  assert(sqlite3_vec_init(db, NULL, NULL) == SQLITE_OK);

  // Use a real SQLite blob/value while deliberately omitting the IN dispatch.
  execute(db, "CREATE TABLE chunks(rowids BLOB); INSERT INTO chunks VALUES(zeroblob(8))");
  sqlite3_stmt *value_stmt = NULL;
  assert(sqlite3_prepare_v2(db, "SELECT 'target'", -1, &value_stmt, NULL) == SQLITE_OK);
  assert(sqlite3_step(value_stmt) == SQLITE_ROW);
  sqlite3_value *value = sqlite3_column_value(value_stmt, 0);
  vec0_vtab p = {0};
  p.db = db; p.schemaName = "main"; p.shadowChunksName = "chunks";
  u8 views[VEC0_METADATA_TEXT_VIEW_BUFFER_LENGTH] = {0};
  u8 bitmap[1] = {0};
  struct Array missing = {0};
  assert(vec0_metadata_filter_text(&p, value, views, 1, VEC0_METADATA_OPERATOR_IN,
                                  bitmap, 0, 1, &missing, 7) == SQLITE_ERROR);
  struct Vec0MetadataIn other = {0};
  other.argv_idx = 8;
  missing.z = &other; missing.length = 1;
  assert(vec0_metadata_filter_text(&p, value, views, 1, VEC0_METADATA_OPERATOR_IN,
                                  bitmap, 0, 1, &missing, 7) == SQLITE_ERROR);
  assert(sqlite3_finalize(value_stmt) == SQLITE_OK);

  sqlite3_blob *blob = NULL;
  assert(sqlite3_blob_open(db, "main", "chunks", "rowids", 1, 1, &blob) == SQLITE_OK);
  float floats[2] = {1.0f, 0.0f};
  i8 integers[2] = {1, 0};
  u8 bits[1] = {1};
  assert(vec0_write_vector_to_vector_blob(blob, 0, floats, 2, SQLITE_VEC_ELEMENT_TYPE_FLOAT32) == SQLITE_OK);
  assert(vec0_write_vector_to_vector_blob(blob, 0, integers, 2, SQLITE_VEC_ELEMENT_TYPE_INT8) == SQLITE_OK);
  assert(vec0_write_vector_to_vector_blob(blob, 0, bits, 8, SQLITE_VEC_ELEMENT_TYPE_BIT) == SQLITE_OK);
  u8 before[8], after[8];
  assert(sqlite3_blob_read(blob, before, 8, 0) == SQLITE_OK);
  assert(vec0_write_vector_to_vector_blob(blob, 0, floats, 2, (enum VectorElementType)999) == SQLITE_ERROR);
  assert(sqlite3_blob_read(blob, after, 8, 0) == SQLITE_OK);
  assert(memcmp(before, after, 8) == 0);
  assert(sqlite3_blob_close(blob) == SQLITE_OK);
  if (argc == 2 && strcmp(argv[1], "--boundary-only") == 0) {
    assert(sqlite3_close(db) == SQLITE_OK);
    puts("sqlite-vec allocation/error boundaries passed");
    return 0;
  }

  sqlite3_module module = vec0Module;
  module.xCreate = capture_create;
  assert(sqlite3_create_module(db, "captured_vec", &module, NULL) == SQLITE_OK);
  execute(db, "CREATE VIRTUAL TABLE vectors USING captured_vec(embedding float[2], label text, chunk_size=8)");
  execute(db, "INSERT INTO vectors(rowid,embedding,label) VALUES(1,'[1,0]','first'),(2,'[0,1]','second')");
  const char *query = "SELECT rowid FROM vectors WHERE embedding MATCH '[0.9,0.1]' AND k=1";
  enum Vec0DistanceMetrics metrics[] = {VEC0_DISTANCE_METRIC_L2, VEC0_DISTANCE_METRIC_L1, VEC0_DISTANCE_METRIC_COSINE};
  for (size_t i = 0; i < sizeof(metrics)/sizeof(metrics[0]); ++i) {
    captured->vector_columns[0].distance_metric = metrics[i];
    nearest(db, query, 1);
  }
  captured->vector_columns[0].distance_metric = VEC0_DISTANCE_METRIC_L2;
  nearest(db, "SELECT rowid FROM vectors WHERE embedding MATCH '[0.9,0.1]' AND k=1 AND label IN ('second')", 2);
  captured->vector_columns[0].distance_metric = (enum Vec0DistanceMetrics)999;
  sqlite3_stmt *bad = NULL;
  assert(sqlite3_prepare_v2(db, query, -1, &bad, NULL) == SQLITE_OK);
  assert(sqlite3_step(bad) == SQLITE_ERROR);
  sqlite3_finalize(bad);
  captured->vector_columns[0].distance_metric = VEC0_DISTANCE_METRIC_L2;
  nearest(db, query, 1); // Failure leaves the same connection usable.

  execute(db, "CREATE VIRTUAL TABLE explicit_pk USING vec0(id TEXT PRIMARY KEY, embedding float[2], chunk_size=8)");
  execute(db, "INSERT INTO explicit_pk VALUES('a','[1,0]')");
  execute(db, "CREATE VIRTUAL TABLE int_vectors USING captured_vec(embedding int8[2], chunk_size=8)");
  execute(db, "INSERT INTO int_vectors VALUES(1,vec_int8('[1,0]')),(2,vec_int8('[0,1]'))");
  const char *int_query = "SELECT rowid FROM int_vectors WHERE embedding MATCH vec_int8('[1,0]') AND k=1";
  for (size_t i = 0; i < sizeof(metrics)/sizeof(metrics[0]); ++i) {
    captured->vector_columns[0].distance_metric = metrics[i];
    nearest(db, int_query, 1);
  }
  captured->vector_columns[0].distance_metric = (enum Vec0DistanceMetrics)999;
  assert(sqlite3_prepare_v2(db, int_query, -1, &bad, NULL) == SQLITE_OK);
  assert(sqlite3_step(bad) == SQLITE_ERROR);
  sqlite3_finalize(bad);
  captured->vector_columns[0].distance_metric = VEC0_DISTANCE_METRIC_L2;
  captured->vector_columns[0].element_type = (enum VectorElementType)999;
  assert(sqlite3_prepare_v2(db, int_query, -1, &bad, NULL) == SQLITE_OK);
  assert(sqlite3_step(bad) == SQLITE_ERROR);
  sqlite3_finalize(bad);
  captured->vector_columns[0].element_type = SQLITE_VEC_ELEMENT_TYPE_INT8;
  nearest(db, int_query, 1);
  execute(db, "CREATE VIRTUAL TABLE bit_vectors USING vec0(embedding bit[8], chunk_size=8)");
  execute(db, "INSERT INTO bit_vectors VALUES(1,vec_bit(X'01')),(2,vec_bit(X'02'))");
  nearest(db, "SELECT rowid FROM bit_vectors WHERE embedding MATCH vec_bit(X'01') AND k=1", 1);
  assert(sqlite3_close(db) == SQLITE_OK);
  puts("sqlite-vec native boundaries and supported SQL passed");
  return 0;
}

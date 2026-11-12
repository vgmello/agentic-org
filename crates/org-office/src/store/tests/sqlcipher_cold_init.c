/* Controlled SQLCipher initialization interleaving; all databases are in memory. */
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include "sqlite3.h"

extern int sqlcipher_extra_init(const char *);
static pthread_mutex_t gate = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t changed = PTHREAD_COND_INITIALIZER;
static int hook_entered, release_hook;

int blocked_cipher_init(const char *arg) {
  pthread_mutex_lock(&gate);
  hook_entered = 1;
  pthread_cond_broadcast(&changed);
  while (!release_hook) pthread_cond_wait(&changed, &gate);
  pthread_mutex_unlock(&gate);
  return sqlcipher_extra_init(arg);
}

static void *first_open(void *unused) {
  (void)unused;
  sqlite3 *db = NULL;
  int rc = sqlite3_open(":memory:", &db);
  if (rc != SQLITE_OK) abort();
  sqlite3_close(db);
  return NULL;
}

static void keyed_probe(void) {
  sqlite3 *db = NULL;
  int initialized = sqlite3_initialize();
  int opened = sqlite3_open(":memory:", &db);
  /* Fixed synthetic key; never print SQL or error text. */
  int keyed = opened == SQLITE_OK ? sqlite3_exec(db,
      "PRAGMA key = \"x'0000000000000000000000000000000000000000000000000000000000000000'\";",
      NULL, NULL, NULL) : opened;
  int cipher = 0;
  sqlite3_stmt *version = NULL;
  if (sqlite3_prepare_v2(db, "PRAGMA cipher_version", -1, &version, NULL) == SQLITE_OK &&
      sqlite3_step(version) == SQLITE_ROW && sqlite3_column_bytes(version, 0) > 0)
    cipher = 1;
  sqlite3_finalize(version);
  sqlite3_close(db);
  printf("initialize=%d open=%d key=%d cipher=%d\n", initialized, opened, keyed, cipher);
  fflush(stdout);
}

int main(void) {
  alarm(20); /* Bound fixture deadlocks; never used to order the interleaving. */
  pthread_t first;
  if (pthread_create(&first, NULL, first_open, NULL)) abort();
  pthread_mutex_lock(&gate);
  while (!hook_entered) pthread_cond_wait(&changed, &gate);
  pthread_mutex_unlock(&gate);
  puts("extra_init=blocked");
  fflush(stdout);

  int command;
  while ((command = getchar()) != EOF && command != 'q') {
    if (command == 'i') {
      pthread_mutex_lock(&gate);
      release_hook = 1;
      pthread_cond_broadcast(&changed);
      pthread_mutex_unlock(&gate);
      pthread_join(first, NULL);
      puts("extra_init=complete");
      fflush(stdout);
    } else if (command == 'k') {
      keyed_probe();
    }
  }
  return 0;
}

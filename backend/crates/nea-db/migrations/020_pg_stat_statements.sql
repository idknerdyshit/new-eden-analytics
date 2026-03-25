-- Enable pg_stat_statements for query performance monitoring.
-- Requires shared_preload_libraries to include pg_stat_statements (set in docker-compose.yml).
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

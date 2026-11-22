# Check representative license terms, target-specific dependencies and sources.
read_notice <- function(name) {
  paste(
    readLines(file.path("notices", name), encoding = "UTF-8", warn = FALSE),
    collapse = "\n"
  )
}

cli <- read_notice("cli.txt")
host <- read_notice("host.txt")
expect_text <- function(text, terms) {
  for (term in terms) {
    if (!grepl(term, text, fixed = TRUE)) stop("Missing notice: ", term)
  }
}
expect_text(
  cli,
  c(
    "strsim 0.11.1",
    "tree-sitter-r authors",
    "Max Brunsfeld",
    "src/unicode/LICENSE",
    "src/portable/endian.h",
    "https://www.unicode.org/Public/17.0.0/ucd/UnicodeData.txt",
    "Danny Guo",
    "Titus Wormer",
    "Akash Kurdekar",
    "find-msvc-tools 0.1.15",
    "rustix 1.1.6",
    "LICENSE-UNICODE",
    "rust-stdlib.html"
  )
)
expect_text(
  host,
  c(
    "tao 0.37.3",
    "dpi 0.1.2",
    "LICENSE-LIBM-MIT",
    "Mozilla Public License",
    "https://static.crates.io/crates/option-ext/option-ext-0.2.0.crate",
    "src/unicode_tables/LICENSE-UNICODE",
    "src/spin/LICENSE",
    "LICENSE-THIRD-PARTY",
    "alloc-stdlib 0.3.0",
    "webview2-com 0.39.1",
    "Copyright (C) Microsoft Corporation",
    "The ANTLR Project",
    "Hikaru Koizumi",
    "vswhom 0.1.0",
    "vswhom-sys 0.1.3",
    "winreg 0.56.0",
    "1.0.3800.47",
    "rust-stdlib.html"
  )
)
stopifnot(!grepl("The ANTLR Project", cli, fixed = TRUE))
expect_text(
  read_notice("rust-stdlib.html"),
  "Copyright notices for The Rust Standard Library"
)

# Exercise the collector with real .crate tarballs in an isolated Cargo cache.
source("scripts/generate-notices.R")
check_collector <- function() {
  workspace <- tempfile()
  dir.create(workspace)
  previous <- setwd(workspace)
  on.exit({
    setwd(previous)
    unlink(workspace, recursive = TRUE)
  })
  collector <- environment()
  environment(notice_files) <- collector
  cargo_home <- workspace
  id <- "notice-fixture-1.0.0"
  cache <- file.path("registry", "cache", "test-registry")
  dir.create(cache, recursive = TRUE)
  dir.create(id)
  writeLines(
    c("[package]", 'name = "notice-fixture"', 'version = "1.0.0"'),
    file.path(id, "Cargo.toml")
  )
  writeLines("Upstream authors", file.path(id, "AUTHORS"))
  archive <- file.path(cache, paste0(id, ".crate"))
  utils::tar(archive, id, compression = "gzip", tar = "internal")
  p <- list(name = "notice-fixture", version = "1.0.0", checksum = sha(archive))
  error <- tryCatch(notice_files(p), error = conditionMessage)
  stopifnot(identical(error, paste0("Missing upstream license; review ", id)))
  writeLines("Reviewed terms", file.path(id, "LICENCE"))
  utils::tar(archive, id, compression = "gzip", tar = "internal")
  error <- tryCatch(notice_files(p), error = conditionMessage)
  stopifnot(identical(
    error,
    paste0("Missing checksummed crates.io archive: ", id)
  ))
  p$checksum <- sha(archive)
  notices <- notice_files(p)
  stopifnot(identical(notices$LICENCE, "Reviewed terms\n"))
  write_checked("check.txt", charToRaw("before"), FALSE)
  error <- tryCatch(
    write_checked("check.txt", charToRaw("after"), TRUE),
    error = conditionMessage
  )
  stopifnot(identical(error, "Stale or missing notices: check.txt"))
}
check_collector()

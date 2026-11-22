check_archive_cache <- function() {
  root <- tempfile("archive cache 日本語 ")
  stopifnot(dir.create(root))
  on.exit(unlink(root, recursive = TRUE))
  subject <- new.env(parent = globalenv())
  sys.source("scripts/build-windows.R", subject)
  sys.source("scripts/runtime-deps.R", subject)
  subject$archive_cache_dir <- function() file.path(root, "cache")
  asset <- file.path(root, "shiny.zip")
  # An actual ZIP with shiny/DESCRIPTION. It needs no zip executable on Windows.
  writeBin(
    jsonlite::base64_dec(
      "UEsDBBQAAAAAAMyEdl0XQnwoHAAAABwAAAARAAAAc2hpbnkvREVTQ1JJUFRJT05QYWNrYWdlOiBzaGlueQpWZXJzaW9uOiAxLjAKUEsBAhQDFAAAAAAAzIR2XRdCfCgcAAAAHAAAABEAAAAAAAAAAAAAAIABAAAAAHNoaW55L0RFU0NSSVBUSU9OUEsFBgAAAAABAAEAPwAAAEsAAAAAAA=="
    ),
    asset
  )
  digest <- unname(tools::sha256sum(asset))
  url <- "https://cran.example/bin/windows/contrib/4.6/shiny_1.0.zip"
  pkg <- list(package = "shiny", version = "1.0")
  target <- list(r_version = "4.6.2", packages = list(pkg), urls = url)
  calls <- new.env()
  calls$n <- 0L
  subject$download_zip <- function(urls, files) {
    calls$n <- calls$n + length(urls)
    stopifnot(all(urls == url), file.copy(asset, files, overwrite = TRUE))
  }
  pins <- subject$pin_packages(target, root)
  stopifnot(identical(unname(pins), digest), calls$n == 1L)
  target$package_sha256 <- pins
  stage <- function(label) {
    dest <- file.path(root, label)
    stopifnot(dir.create(dest))
    subject$stage_packages(target, root, dest)
    stopifnot(file.exists(file.path(dest, "library/shiny/DESCRIPTION")))
  }
  stage("first build")
  stage("second build")
  stopifnot(calls$n == 1L)
  object <- file.path(root, "cache", paste0(digest, ".zip"))
  stopifnot(file.exists(object))
  writeLines("corrupted cache", object)
  stage("repair")
  stopifnot(calls$n == 2L, unname(tools::sha256sum(object)) == digest)
  # A fresh download has to match the lock even when the ZIP declares the right version.
  unlink(object)
  target$package_sha256 <- paste(rep("0", 64L), collapse = "")
  expect_bundle_error(stage("mismatch"), "SHA-256 mismatch for shiny")
  stopifnot(!length(list.files(dirname(object), pattern = "[.]zip$")))
  target$package_sha256 <- pins
  download <- subject$download_zip
  writeLines("previously damaged object", object)
  subject$download_zip <- function(urls, files) {
    writeLines("partial", files)
    stop("interrupted transfer")
  }
  expect_bundle_error(stage("interrupt"), "interrupted transfer")
  stopifnot(!length(list.files(dirname(object), pattern = "[.]zip$")))
  subject$download_zip <- download
  stage("recover")
  # Locks without SHA-256 continue to fetch and check DESCRIPTION on every build.
  target$package_sha256 <- NULL
  count <- calls$n
  stage("legacy")
  stage("old managed")
  stopifnot(calls$n == count + 2L)
  target$packages[[1]]$version <- "2.0"
  expect_bundle_error(subject$pin_packages(target, root), "shiny")
}
check_archive_cache()

check_cache_workers <- function() {
  root <- tempfile("two cache writers ")
  stopifnot(dir.create(root))
  on.exit(unlink(root, recursive = TRUE))
  fixture <- file.path(root, "download.zip")
  writeBin(charToRaw("shared archive bytes"), fixture)
  sum <- unname(tools::sha256sum(fixture))
  workers <- parallel::makePSOCKcluster(2L)
  on.exit(parallel::stopCluster(workers), add = TRUE, after = FALSE)
  readbacks <- parallel::clusterCall(
    workers,
    function(script, dir, fixture, sum) {
      subject <- new.env()
      sys.source(script, subject)
      dest <- file.path(dir, paste0("private-", Sys.getpid()))
      subject$archive_cache_dir <- function() file.path(dir, "shared cache")
      subject$download_zip <- function(urls, files) {
        writeLines("requested", paste0(dest, ".request"))
        Sys.sleep(0.2)
        stopifnot(file.copy(fixture, files, overwrite = TRUE))
      }
      subject$fetch_archives("https://fixture.invalid/shared.zip", dest, sum)
      unname(tools::sha256sum(dest))
    },
    normalizePath("scripts/build-windows.R"),
    root,
    fixture,
    sum
  )
  stopifnot(
    identical(unlist(readbacks), rep(sum, 2L)),
    length(list.files(root, pattern = "[.]request$")) == 1L
  )
}
check_cache_workers()

check_package_pin_table <- function() {
  path <- tempfile()
  on.exit(unlink(path))
  old <- legacy_minimal_lock()
  writeLines(old, path)
  stopifnot(is.null(windows_target(path)$package_sha256))
  lines <- readLines("bundles/minimal/rproj.lock")
  writeLines(lines, path)
  pins <- windows_target(path)$package_sha256
  stopifnot(length(pins) == 30L, all(vapply(pins, valid_sha256, FALSE)))
  other <- old[seq.int(match("[[targets]]", old), length(old))]
  other <- sub(
    'r_version = "4.6.2"',
    'r_version = "4.5.3"',
    other,
    fixed = TRUE
  )
  writeLines(
    c(old, other, lines[seq.int(match("[shinytauri]", lines), length(lines))]),
    path
  )
  expect_bundle_error(windows_target(path, "4.6.2"), "package SHA-256")
  for (bad in list(
    lines[!startsWith(lines, '"shiny" =')],
    lines[!startsWith(lines, "archive_format")],
    c(lines, sprintf('"Extra" = "%s"', pins[[1]])),
    sub("archive_format = 1", "archive_format = 2", lines),
    sub('^"shiny" = .*$', '"shiny" = "invalid"', lines)
  )) {
    writeLines(bad, path)
    expect_bundle_error(windows_target(path), "package SHA-256")
  }
}
check_package_pin_table()

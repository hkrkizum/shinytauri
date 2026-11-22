# Exercise the real staging and unzip paths with a tiny Windows-layout ZIP.
check_portable_runtime <- function() {
  root <- tempfile("portable R ")
  stopifnot(dir.create(root))
  on.exit(unlink(root, recursive = TRUE))
  subject <- new.env(parent = globalenv())
  sys.source("scripts/build-windows.R", subject)
  subject$archive_cache_dir <- function() file.path(root, "cache")
  version <- "4.5.3"
  asset <- "portable-r-4.5.3-win-x64.zip"
  url <- paste0(
    "https://github.com/portable-r/portable-r-windows/releases/download/v4.5.3/",
    asset
  )
  archive <- file.path(root, asset)
  writeBin(
    jsonlite::base64_dec(paste0(
      "UEsDBBQAAAAAAAAAIQDuQOUFBwAAAAcAAAAqAAAAcG9ydGFibGUtci00LjUuMy13aW4t",
      "eDY0L2Jpbi94NjQvUnRlcm0uZXhlZml4dHVyZVBLAQIUAxQAAAAAAAAAIQDuQOUFBwAA",
      "AAcAAAAqAAAAAAAAAAAAAACAAQAAAABwb3J0YWJsZS1yLTQuNS4zLXdpbi14NjQvYmlu",
      "L3g2NC9SdGVybS5leGVQSwUGAAAAAAEAAQBYAAAATwAAAAAA"
    )),
    archive
  )
  digest <- unname(tools::sha256sum(archive))
  record <- paste(digest, asset, sep = "  ")
  calls <- new.env(parent = emptyenv())
  calls$urls <- character()
  subject$download_zip <- function(remote, file) {
    calls$urls <- c(calls$urls, remote)
    if (identical(remote, paste0(url, ".sha256"))) {
      writeBin(charToRaw(paste0(record, "\r\n")), file)
    } else {
      stopifnot(
        identical(remote, url),
        file.copy(archive, file, overwrite = TRUE)
      )
    }
  }
  stopifnot(
    identical(
      subject$portable_r_digest("4.6.2"),
      unname(runtime_sha256[["4.6.2"]])
    ),
    !length(calls$urls)
  )
  for (bad in list("latest", "4.5", "4.5.03", "../other", "4.5.3\n", 4.5)) {
    expect_bundle_error(subject$portable_r_digest(bad), "major.minor.patch")
  }
  stopifnot(
    !length(calls$urls),
    identical(subject$portable_r_digest(version), digest),
    identical(calls$urls, paste0(url, ".sha256"))
  )
  target <- list(r_version = version)
  payload <- file.path(root, "payload")
  stopifnot(dir.create(payload))
  # Unmanaged lock: validate the exact public checksum record before requesting the ZIP.
  for (bad in c(
    paste(digest, "portable-r-4.5.3-win-aarch64.zip", sep = "  "),
    paste(digest, asset, sep = "  /"),
    paste(record, record, sep = "\n"),
    paste("sha256", asset, sep = "  ")
  )) {
    calls$urls <- character()
    record <- bad
    expect_bundle_error(
      subject$stage_runtime(target, root, payload),
      "checksum"
    )
    stopifnot(
      identical(calls$urls, paste0(url, ".sha256")),
      !length(list.files(payload))
    )
  }
  download <- subject$download_zip
  subject$download_zip <- function(remote, file) {
    stopifnot(identical(remote, paste0(url, ".sha256")))
    stop("release not found")
  }
  expect_bundle_error(
    subject$stage_runtime(target, root, payload),
    "x64 checksum: release not found"
  )
  stopifnot(!length(list.files(payload)))
  subject$download_zip <- download
  # CLI-managed builds must not adopt a changed online checksum on mismatch.
  target$runtime_sha256 <- paste(rep("0", 64L), collapse = "")
  calls$urls <- character()
  expect_bundle_error(
    subject$stage_runtime(target, root, payload),
    "SHA-256 mismatch"
  )
  stopifnot(identical(calls$urls, url), !length(list.files(payload)))
  target$runtime_sha256 <- digest
  calls$urls <- character()
  subject$stage_runtime(target, root, payload)
  stopifnot(
    identical(calls$urls, url),
    rawToChar(readBin(
      file.path(payload, "R/bin/x64/Rterm.exe"),
      "raw",
      n = 7L
    )) ==
      "fixture"
  )
}

check_portable_runtime()

check_windows_selection <- function() {
  path <- tempfile()
  on.exit(unlink(path))
  # A rig lock can contain a Linux target and multiple Windows R versions.
  base <- legacy_minimal_lock()
  stanza <- base[seq.int(match("[[targets]]", base), length(base))]
  other <- sub(
    'r_version = "4.6.2"',
    'r_version = "4.5.3"',
    stanza,
    fixed = TRUE
  )
  writeLines(c(base, other), path)
  expect_bundle_error(windows_target(path, "4.5.3"), "Windows binary")
  writeLines(
    c(
      base,
      gsub("/4.6/", "/4.5/", other, fixed = TRUE),
      sub(
        "x86_64-w64-mingw32",
        "x86_64-unknown-linux-gnu",
        stanza,
        fixed = TRUE
      )
    ),
    path
  )
  expect_bundle_error(windows_target(path), "rVersion")
  stopifnot(identical(windows_target(path, "4.5.3")$r_version, "4.5.3"))
  expect_bundle_error(windows_target(path, "4.5.4"), "4.5.4")
  writeLines(c(base, stanza), path)
  expect_bundle_error(windows_target(path, "4.6.2"), "4.6.2")
  writeLines(c(base, "[shinytauri]", 'runtime_sha256 = "not-a-digest"'), path)
  expect_bundle_error(windows_target(path), "SHA-256")
  writeLines(
    c(
      base,
      "[shinytauri]",
      sprintf('runtime_sha256 = "%s"', paste(rep("0", 64), collapse = ""))
    ),
    path
  )
  expect_bundle_error(windows_target(path), "4.6.2")
}
check_windows_selection()

check_changed_config <- function() {
  root <- tempfile()
  stopifnot(dir.create(file.path(root, ".shinytauri"), recursive = TRUE))
  on.exit(unlink(root, recursive = TRUE))
  profile <- file.path(root, "shinytauri.toml")
  lock <- file.path(root, ".shinytauri", "windows.lock")
  stopifnot(file.copy("bundles/minimal/rproj.lock", lock))
  writeLines(
    c(
      'app = "."',
      'files = ["app.R"]',
      'lock = ".shinytauri/windows.lock"',
      'rVersion = "4.6.1"'
    ),
    profile
  )
  expect_bundle_error(build_windows(profile), "run shinytauri lock again")
}
check_changed_config()

source("scripts/runtime-deps.R")

check_runtime_inputs <- function() {
  project <- tempfile("non-rig app ")
  stopifnot(dir.create(project))
  on.exit(unlink(project, recursive = TRUE))
  metadata <- file.path(project, "DESCRIPTION")
  writeLines(
    c(
      "Package: sample",
      "Imports: shiny, stringr (>= 1.0.0)",
      "Suggests: testthat"
    ),
    metadata
  )
  spec <- list(dependencyFile = metadata, packages = "stringr=1.6.0")
  req <- runtime_requirements(spec)
  stopifnot(
    identical(sort(unique(req$name)), c("shiny", "stringr")),
    "1.6.0" %in% req$version
  )
  expect_bundle_error(
    check_resolved_versions(req, windows_target("bundles/minimal/rproj.lock")),
    "stringr"
  )
  digest <- runtime_fingerprint(spec)
  cat("Remotes: github::example/sample\n", file = metadata, append = TRUE)
  stopifnot(!identical(digest, runtime_fingerprint(spec, "4.6.2")))
  expect_bundle_error(runtime_requirements(spec), "Remotes")
  writeLines("Package: sample\nImports: injected (>x)", metadata)
  expect_bundle_error(runtime_requirements(spec), "dependency")
  expect_bundle_error(
    runtime_requirements(list(packages = "github::private/package")),
    "NAME"
  )

  input <- file.path(project, "renv.lock")
  cran <- list(Name = "CRAN", URL = "https://cloud.r-project.org")
  record <- list(
    Package = "shiny",
    Version = "1.14.0",
    Source = "Repository",
    Repository = "CRAN"
  )
  lock <- list(
    R = list(Version = "4.6.2", Repositories = list(cran)),
    Packages = list(shiny = record)
  )
  jsonlite::write_json(lock, input, auto_unbox = TRUE)
  stopifnot(identical(
    renv_requirements(input),
    c("R (== 4.6.2)", "shiny (== 1.14.0)")
  ))
  lock$Packages$shiny$Repository <- "RSPM"
  jsonlite::write_json(lock, input, auto_unbox = TRUE)
  stopifnot(identical(
    renv_requirements(input),
    c("R (== 4.6.2)", "shiny (== 1.14.0)")
  ))
  lock$R$Repositories[[1L]]$URL <- "https://private.invalid"
  jsonlite::write_json(lock, input, auto_unbox = TRUE)
  expect_bundle_error(renv_requirements(input), "repository")
  lock$R$Repositories <- list(cran)
  lock$Packages$shiny$Source <- "GitHub"
  jsonlite::write_json(lock, input, auto_unbox = TRUE)
  expect_bundle_error(renv_requirements(input), "shiny")
  lock$R$Version <- NULL
  jsonlite::write_json(lock, input, auto_unbox = TRUE)
  expect_bundle_error(renv_requirements(input), "version")
}

check_minimal_preflight <- function() {
  project <- tempfile("plain app ")
  stopifnot(dir.create(project))
  on.exit(unlink(project, recursive = TRUE))
  target <- windows_target("bundles/minimal/rproj.lock")
  script <- file.path(project, "app.R")
  writeLines(
    c(
      "# stringr:: is just a comment",
      "library(shiny)",
      "print('DT::datatable')"
    ),
    script
  )
  check_minimal_imports(project, target)
  writeLines("stringr::str_pad('x', 3)", script)
  expect_bundle_error(check_minimal_imports(project, target), "stringr")
  writeLines('function(x, y = stringr::str_to_upper("ok")) y', script)
  expect_bundle_error(check_minimal_imports(project, target), "stringr")
  writeLines("library(pkg, character.only = TRUE)", script)
  expect_bundle_error(check_minimal_imports(project, target), "dynamic")
}
check_runtime_inputs()
check_minimal_preflight()

check_old_minimal_profile <- function() {
  app <- tempfile()
  support <- file.path(app, ".shinytauri")
  dir.create(support, recursive = TRUE)
  on.exit(unlink(app, recursive = TRUE))
  lock <- file.path(support, "rproj.lock")
  stopifnot(file.copy("bundles/minimal/rproj.lock", lock))
  profile <- file.path(app, "shinytauri.toml")
  writeLines(
    c('app = "."', 'lock = ".shinytauri/rproj.lock"', 'files = ["app.R"]'),
    profile
  )
  stopifnot(isTRUE(read_bundle(profile)$checkMinimal))
  cat("\n# independently maintained\n", file = lock, append = TRUE)
  stopifnot(!isTRUE(read_bundle(profile)$checkMinimal))
}
check_old_minimal_profile()

check_committed_windows_lock <- function() {
  tmp <- tempfile()
  dir.create(tmp)
  on.exit(unlink(tmp, recursive = TRUE))
  input <- file.path(tmp, "DESCRIPTION")
  lock <- file.path(tmp, "windows.lock")
  spec <- list(lock = lock, dependencyFile = input)
  writeBin(charToRaw("Package: sample\nImports: shiny\n"), input)
  digest <- runtime_fingerprint(spec)
  stopifnot(file.copy("bundles/minimal/rproj.lock", lock))
  cat(
    sprintf('\n[shinytauri]\ninput_sha256 = "%s"\n', digest),
    file = lock,
    append = TRUE
  )
  verify_runtime_lock(spec)
  writeBin(charToRaw("Package: sample\r\nImports: shiny\r\n"), input)
  verify_runtime_lock(spec)
  writeLines("Package: sample\nImports: shiny, DT", input)
  expect_bundle_error(verify_runtime_lock(spec), "shinytauri lock")
}
check_committed_windows_lock()

check_runtime_selection <- function() {
  tmp <- tempfile()
  stopifnot(dir.create(tmp))
  on.exit(unlink(tmp, recursive = TRUE))
  spec <- list(packages = "shiny")
  stopifnot(
    identical(runtime_version(spec), "4.6.2"),
    identical(runtime_version(c(spec, list(rVersion = "4.5.3"))), "4.5.3"),
    !identical(
      runtime_fingerprint(spec),
      runtime_fingerprint(c(spec, list(rVersion = "4.5.3")))
    )
  )
  description <- file.path(tmp, "DESCRIPTION")
  writeLines(
    "Package: demo\nDepends: R (== 4.5.3)\nImports: shiny",
    description
  )
  spec <- list(dependencyFile = description)
  stopifnot(identical(runtime_version(spec), "4.5.3"))
  expect_bundle_error(
    runtime_version(c(spec, list(rVersion = "4.6.2"))),
    "4.5.3"
  )
  writeLines("Package: demo\nDepends: R (>= 4.5.3), R (< 4.6.0)", description)
  expect_bundle_error(runtime_version(spec), "4.6.0")
  stopifnot(identical(
    runtime_version(c(spec, list(rVersion = "4.5.3"))),
    "4.5.3"
  ))
  file <- file.path(tmp, "renv.lock")
  lock <- list(
    R = list(Version = "4.5.3"),
    Packages = list(
      shiny = list(
        Package = "shiny",
        Source = "Repository",
        Repository = "CRAN",
        Version = "1.12.1"
      )
    )
  )
  jsonlite::write_json(lock, file, auto_unbox = TRUE)
  spec <- list(dependencyFile = file)
  stopifnot(identical(runtime_version(spec), "4.5.3"))
  expect_bundle_error(
    runtime_version(c(spec, list(rVersion = "4.6.2"))),
    "4.5.3"
  )
}
check_runtime_selection()

check_lock_pins_runtime <- function() {
  root <- tempfile()
  stopifnot(dir.create(root))
  on.exit(unlink(root, recursive = TRUE))
  spec <- list(
    managed = TRUE,
    packages = "shiny",
    rVersion = "4.5.3",
    lock = file.path(root, ".shinytauri/windows.lock")
  )
  # Exercise the lock writer without downloading metadata or packages in unit tests.
  # The fixture stands in for rig's output and is still checked by windows_target.
  fixture <- readLines("bundles/minimal/rproj.lock")
  digest <- paste(rep("a", 64L), collapse = "")
  subject <- new.env(parent = environment())
  sys.source("scripts/runtime-deps.R", envir = subject)
  subject$portable_r_digest <- function(version) {
    stopifnot(identical(version, spec$rVersion))
    digest
  }
  subject$system2 <- function(command, args) {
    stopifnot(identical(command, "rig"), identical(args, c("proj", "lock")))
    manifest <- RcppTOML::parseTOML("rproj.toml")
    stopifnot(
      identical(manifest$dependencies$R, "== 4.5.3"),
      identical(manifest$tool$rig[["r-versions"]], "4.5.3")
    )
    lines <- sub(
      'r_version = "4.6.2"',
      'r_version = "4.5.3"',
      fixture,
      fixed = TRUE
    )
    writeLines(gsub("/4.6/", "/4.5/", lines, fixed = TRUE), "rproj.lock")
    0L
  }
  subject$lock_windows(spec)
  recorded <- RcppTOML::parseTOML(spec$lock)$shinytauri
  stopifnot(identical(recorded$runtime_sha256, digest))
  verify_runtime_lock(spec)
  lines <- readLines(spec$lock)
  writeLines(lines[!startsWith(lines, "runtime_sha256 =")], spec$lock)
  expect_bundle_error(verify_runtime_lock(spec), "SHA-256")
  writeLines(lines, spec$lock)
  spec$rVersion <- "4.6.2"
  expect_bundle_error(verify_runtime_lock(spec), "shinytauri lock")
}
check_lock_pins_runtime()

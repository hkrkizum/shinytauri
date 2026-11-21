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
  stopifnot(!identical(digest, runtime_fingerprint(spec)))
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
  stopifnot(identical(renv_requirements(input), "shiny (== 1.14.0)"))
  lock$Packages$shiny$Repository <- "RSPM"
  jsonlite::write_json(lock, input, auto_unbox = TRUE)
  stopifnot(identical(renv_requirements(input), "shiny (== 1.14.0)"))
  lock$R$Repositories[[1L]]$URL <- "https://private.invalid"
  jsonlite::write_json(lock, input, auto_unbox = TRUE)
  expect_bundle_error(renv_requirements(input), "repository")
  lock$R$Repositories <- list(cran)
  lock$Packages$shiny$Source <- "GitHub"
  jsonlite::write_json(lock, input, auto_unbox = TRUE)
  expect_bundle_error(renv_requirements(input), "shiny")
  lock$R$Version <- "4.1.0"
  jsonlite::write_json(lock, input, auto_unbox = TRUE)
  expect_bundle_error(renv_requirements(input), "4.1.0")
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

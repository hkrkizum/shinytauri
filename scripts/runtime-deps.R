# Resolve only redistribution dependencies. The user's R library is never used.
runtime_version <- function() names(runtime_sha256)[[1L]]

package_pattern <- "[A-Za-z][A-Za-z0-9.]*"
version_pattern <- "[0-9]+([.-][0-9]+)*"

base_packages <- function() {
  c(
    "R",
    rownames(utils::installed.packages(lib.loc = .Library, priority = "base"))
  )
}

description_requirements <- function(path) {
  records <- read.dcf(path)
  if (nrow(records) != 1L) {
    stop("Expected one package DESCRIPTION: ", path)
  }
  if (any(c("Remotes", "Additional_repositories") %in% colnames(records))) {
    stop(
      "DESCRIPTION contains Remotes or Additional_repositories; only public CRAN binaries are supported."
    )
  }
  fields <- intersect(c("Depends", "Imports", "LinkingTo"), colnames(records))
  trimws(unlist(strsplit(records[1L, fields], ",", fixed = TRUE)))
}

renv_requirements <- function(path) {
  lock <- jsonlite::read_json(path, simplifyVector = FALSE)
  if (!identical(lock$R$Version, runtime_version())) {
    stop(
      "renv.lock requires R ",
      lock$R$Version,
      "; supported portable R: ",
      runtime_version()
    )
  }
  public_url <- paste0(
    "^https://(cloud[.]r-project[.]org|cran[.]r-project[.]org|cran[.]rstudio[.]com)/?$|",
    "^https://(packagemanager[.]posit[.]co|p3m[.]dev)/cran/(__linux__/[a-z0-9_-]+/)?(latest|[0-9]{4}-[0-9]{2}-[0-9]{2})/?$"
  )
  repositories <- lock$R$Repositories
  for (repo in repositories) {
    if (
      !is.character(repo$URL) ||
        length(repo$URL) != 1L ||
        !grepl(public_url, repo$URL)
    ) {
      stop("Unsupported repository URL in renv.lock: ", repo$URL)
    }
  }
  # renv snapshots of public Posit Linux binaries commonly say RSPM in each
  # record while recording only CRAN in R.Repositories.
  allowed <- "CRAN"
  if (length(repositories)) {
    aliases <- vapply(repositories, function(repo) repo$Name, "")
    allowed <- c(allowed, aliases, "RSPM", "PPM")
  }
  records <- lock$Packages
  if (
    is.null(names(records)) ||
      !"shiny" %in% names(records) ||
      anyDuplicated(names(records))
  ) {
    stop("renv.lock must contain named, unique Packages including shiny.")
  }
  vapply(
    names(records),
    function(name) {
      pkg <- records[[name]]
      if (
        !identical(pkg$Package, name) ||
          !identical(pkg$Source, "Repository") ||
          !is.character(pkg$Repository) ||
          length(pkg$Repository) != 1L ||
          !pkg$Repository %in% allowed ||
          any(grepl("^(Remote|Path$)", names(pkg)))
      ) {
        stop(
          "Unsupported renv source for ",
          name,
          ": expected a public CRAN Repository record."
        )
      }
      if (
        !is.character(pkg$Version) ||
          length(pkg$Version) != 1L ||
          !grepl(paste0("^", version_pattern, "$"), pkg$Version) ||
          !grepl(paste0("^", package_pattern, "$"), name)
      ) {
        stop("Invalid package name or version in renv.lock: ", name)
      }
      sprintf("%s (== %s)", name, pkg$Version)
    },
    character(1),
    USE.NAMES = FALSE
  )
}

runtime_requirements <- function(spec) {
  terms <- "shiny"
  if (!is.null(spec$dependencyFile)) {
    source <- switch(
      basename(spec$dependencyFile),
      DESCRIPTION = description_requirements,
      renv.lock = renv_requirements,
      stop("dependencyFile must point to DESCRIPTION or renv.lock.")
    )
    terms <- c(terms, source(spec$dependencyFile))
  }
  manual <- spec$packages
  if (!is.null(manual)) {
    if (
      !is.character(manual) ||
        !length(manual) ||
        !all(
          grepl(
            paste0("^", package_pattern, "(=", version_pattern, ")?$"),
            manual
          )
        )
    ) {
      stop("Specify each package as a CRAN NAME or NAME=VERSION in packages.")
    }
    terms <- c(terms, sub("=([^=]+)$", " (== \\1)", manual))
  }
  pattern <- paste0(
    "^(",
    package_pattern,
    ")(?:\\s*\\(\\s*(>=|<=|==|>|<)\\s*(",
    version_pattern,
    ")\\s*\\))?$"
  )
  matches <- regmatches(terms, regexec(pattern, terms, perl = TRUE))
  if (any(lengths(matches) == 0L)) {
    stop(
      "Unsupported DESCRIPTION dependency: ",
      paste(terms[lengths(matches) == 0L], collapse = ", ")
    )
  }
  data.frame(
    name = vapply(matches, `[[`, "", 2L),
    op = vapply(matches, `[[`, "", 3L),
    version = vapply(matches, `[[`, "", 4L)
  )
}

check_resolved_versions <- function(requirements, target) {
  versions <- vapply(target$packages, function(p) p$version, "")
  names(versions) <- vapply(target$packages, function(p) p$package, "")
  versions[base_packages()] <- target$r_version
  for (i in seq_len(nrow(requirements))) {
    req <- requirements[i, ]
    version <- unname(versions[req$name])
    if (is.na(version)) {
      stop("Missing resolved package: ", req$name)
    }
    if (!nzchar(req$op)) {
      next
    }
    cmp <- utils::compareVersion(version, req$version)
    valid <- switch(
      req$op,
      "==" = identical(version, req$version),
      ">=" = cmp >= 0L,
      "<=" = cmp <= 0L,
      ">" = cmp > 0L,
      "<" = cmp < 0L
    )
    if (!valid) {
      stop(
        "Windows version conflicts with requested ",
        req$name,
        " ",
        req$op,
        " ",
        req$version
      )
    }
  }
}

runtime_fingerprint <- function(spec) {
  input <- spec$dependencyFile
  temp <- tempfile()
  on.exit(unlink(temp))
  source <- NULL
  if (!is.null(input)) {
    # The same committed text may be checked out with LF or CRLF.
    bytes <- readBin(input, "raw", n = file.size(input))
    text <- gsub("\r\n", "\n", rawToChar(bytes), fixed = TRUE, useBytes = TRUE)
    writeBin(charToRaw(text), temp)
    source <- c(basename(input), unname(tools::sha256sum(temp)))
  }
  lines <- c(
    "windows-cran-v1",
    runtime_version(),
    source,
    sort(unique(spec$packages %||% character()), method = "radix")
  )
  writeBin(charToRaw(paste(lines, collapse = "\n")), temp)
  unname(tools::sha256sum(temp))
}

verify_runtime_lock <- function(spec, target = windows_target(spec$lock)) {
  lock <- RcppTOML::parseTOML(spec$lock)
  if (!identical(lock$shinytauri$input_sha256, runtime_fingerprint(spec))) {
    stop("Windows dependencies have changed; run shinytauri lock again.")
  }
  check_resolved_versions(runtime_requirements(spec), target)
}

lock_windows <- function(spec) {
  if (!spec$managed) {
    stop(
      "This config uses an existing rig lock. For CLI-managed dependencies set packages or dependencyFile and lock = '.shinytauri/windows.lock'."
    )
  }
  fingerprint <- runtime_fingerprint(spec)
  req <- runtime_requirements(spec)
  pkg <- req[!req$name %in% base_packages(), ]
  dependencies <- vapply(
    split(pkg, pkg$name),
    function(r) {
      constrained <- r[nzchar(r$op), ]
      constraint <- if (nrow(constrained)) {
        paste(
          unique(paste(constrained$op, constrained$version)),
          collapse = ", "
        )
      } else {
        "*"
      }
      sprintf('"%s" = "%s"', r$name[[1L]], constraint)
    },
    ""
  )
  dir <- tempfile("windows-lock-")
  stopifnot(dir.create(dir))
  old <- setwd(dir)
  on.exit({
    setwd(old)
    unlink(dir, recursive = TRUE)
  })
  writeLines(
    c(
      '[project]',
      'name = "shiny-runtime"',
      'version = "0.1.0"',
      'type = "project"',
      '[dependencies]',
      sprintf('R = "== %s"', runtime_version()),
      unname(dependencies),
      '[tool.rig]',
      sprintf('r-versions = ["%s"]', runtime_version()),
      sprintf('platforms = ["%s"]', platform),
      'prefer-binary = true',
      '[[repository]]',
      'name = "bioc"',
      'enabled = false'
    ),
    "rproj.toml"
  )
  if (system2("rig", c("proj", "lock")) != 0L) {
    stop("Could not resolve Windows CRAN binary versions.")
  }
  target <- windows_target("rproj.lock")
  check_resolved_versions(req, target)
  if (!identical(fingerprint, runtime_fingerprint(spec))) {
    stop("Dependency inputs changed while locking; retry.")
  }
  parent <- dirname(spec$lock)
  if (!dir.exists(parent) && !dir.create(parent) && !dir.exists(parent)) {
    stop("Cannot create ", parent)
  }
  if (
    !identical(
      absolute_path(parent),
      file.path(absolute_path(dirname(parent)), basename(parent))
    ) ||
      (file.exists(spec$lock) &&
        is.null(RcppTOML::parseTOML(spec$lock)$shinytauri$input_sha256))
  ) {
    stop(
      "Refusing to replace a lock or directory not owned by shinytauri: ",
      spec$lock
    )
  }
  staged <- tempfile(".windows-", tmpdir = parent)
  on.exit(unlink(staged), add = TRUE)
  stopifnot(file.copy("rproj.lock", staged))
  cat(
    sprintf('\n[shinytauri]\ninput_sha256 = "%s"\n', fingerprint),
    file = staged,
    append = TRUE
  )
  stopifnot(file.rename(staged, spec$lock))
  message("Windows runtime lock: ", spec$lock)
}

# A minimal app can build immediately. Inspect literal imports without sourcing
# any app code; dynamic imports require a dependency declaration from the author.
check_minimal_imports <- function(app, target) {
  available <- c(
    base_packages(),
    vapply(target$packages, function(p) p$package, "")
  )
  guidance <- ". Declare runtime dependencies in DESCRIPTION or packages, then run shinytauri lock."
  scripts <- list.files(
    app,
    full.names = TRUE,
    recursive = TRUE,
    all.files = TRUE
  )
  if (any(grepl("[.](rmd|qmd)$", scripts, ignore.case = TRUE))) {
    stop("Cannot infer dependencies of embedded document code", guidance)
  }
  inspect <- function(expr) {
    if (is.pairlist(expr)) {
      return(invisible(lapply(expr, inspect)))
    }
    if (!is.call(expr)) {
      return(NULL)
    }
    fun <- expr[[1L]]
    if (
      is.call(fun) &&
        length(fun) == 3L &&
        (identical(fun[[1L]], quote(`::`)) ||
          identical(fun[[1L]], quote(`:::`)))
    ) {
      fun <- fun[[3L]]
    }
    name <- if (is.symbol(fun)) as.character(fun) else ""
    namespaced <- name %in% c("::", ":::")
    imports <- c(
      "library",
      "require",
      "requireNamespace",
      "loadNamespace",
      "getNamespace",
      "asNamespace",
      "getExportedValue"
    )
    if ((namespaced || name %in% imports) && length(expr) > 1L) {
      args <- as.list(expr)[-1L]
      value <- args[["package"]] %||% args[[1L]]
      literal_name <- namespaced ||
        (name %in%
          c("library", "require") &&
          (is.null(args[["character.only"]]) ||
            identical(args[["character.only"]], FALSE)))
      if (is.symbol(value) && literal_name) {
        value <- as.character(value)
      }
      if (!is.character(value) || length(value) != 1L) {
        stop("Cannot infer dynamic package import", guidance)
      }
      if (!value %in% available) {
        stop("Missing package ", value, " in the Windows runtime", guidance)
      }
    }
    invisible(lapply(as.list(expr), inspect))
  }
  for (path in scripts[grepl("[.]r$", scripts, ignore.case = TRUE)]) {
    lapply(parse(path, keep.source = FALSE), inspect)
  }
  invisible(NULL)
}

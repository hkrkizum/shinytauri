# Pandoc can put entire stylesheets and ES modules behind data: URLs.
# Decode these as UTF-8 text; binary image and font URLs stay embedded.
decode_uri <- function(uri) {
  comma <- regexpr(",", uri, fixed = TRUE)
  stopifnot(comma > 0L)
  header <- tolower(substr(uri, 1L, comma - 1L))
  body <- substring(uri, comma + 1L)
  value <- if (endsWith(header, ";base64")) {
    rawToChar(base64enc::base64decode(body))
  } else {
    curl::curl_unescape(body)
  }
  Encoding(value) <- "UTF-8"
  value
}

inline_imports <- function(css) {
  # Quarto/Pandoc emit quoted URLs or url(...), sometimes with media conditions.
  pattern <- paste0(
    "(?i)@import\\s+(?:url\\(\\s*)?[\"']?",
    "(data:text/css[^[:space:]\"'()]+)[\"']?\\s*\\)?\\s*([^;{}]*);"
  )
  matches <- gregexpr(pattern, css, perl = TRUE)
  rules <- regmatches(css, matches)[[1L]]
  if (length(rules)) {
    expand <- function(rule) {
      parts <- regmatches(rule, regexec(pattern, rule, perl = TRUE))[[1L]]
      nested <- inline_imports(decode_uri(parts[[2L]]))
      media <- trimws(parts[[3L]])
      if (nzchar(media)) paste("@media", media, "{", nested, "}") else nested
    }
    regmatches(css, matches) <- list(vapply(rules, expand, character(1)))
  }
  css
}

#' @export
inline_resources <- function(input, output) {
  doc <- xml2::read_html(
    input,
    encoding = "UTF-8",
    options = c("HUGE", "NONET")
  )
  for (node in xml2::xml_find_all(doc, "//link[@rel='stylesheet'][@href]")) {
    uri <- xml2::xml_attr(node, "href")
    stopifnot(startsWith(uri, "data:text/css"))
    xml2::xml_name(node) <- "style"
    xml2::xml_attr(node, "href") <- NULL
    xml2::xml_attr(node, "rel") <- NULL
    xml2::xml_text(node) <- decode_uri(uri)
  }
  for (node in xml2::xml_find_all(doc, "//style")) {
    xml2::xml_text(node) <- inline_imports(xml2::xml_text(node))
  }
  for (node in xml2::xml_find_all(doc, "//script[@src]")) {
    uri <- xml2::xml_attr(node, "src")
    stopifnot(grepl("^data:(application|text)/javascript[,;]", uri))
    xml2::xml_attr(node, "src") <- NULL
    xml2::xml_text(node) <- decode_uri(uri)
  }
  xml2::write_html(doc, output, encoding = "UTF-8", options = "as_html")
}

#' @export
dashboard <- function(counter, counts, quarto) {
  stopifnot(length(counter) == 1L, length(counts) == 11L, sum(counts) > 0L)
  work <- tempfile("dice-report-")
  stopifnot(dir.create(work))
  on.exit(unlink(work, recursive = TRUE), add = TRUE)
  sums <- 2:12
  n <- sum(counts)
  rows <- sprintf(
    "| %d | %d | %.1f %% | %.1f %% |",
    sums,
    counts,
    100 * counts / n,
    100 * (6 - abs(sums - 7)) / 36
  )
  qmd <- readLines(box::file("dashboard.qmd"), encoding = "UTF-8")
  qmd <- sub("{{COUNTER}}", as.character(counter), qmd, fixed = TRUE)
  qmd <- sub(
    "{{TRIALS}}",
    format(n, big.mark = ",", scientific = FALSE),
    qmd,
    fixed = TRUE
  )
  qmd <- sub(
    "{{SEVEN}}",
    sprintf("%.1f", 100 * counts[[6L]] / n),
    qmd,
    fixed = TRUE
  )
  qmd <- sub("{{ROWS}}", paste(rows, collapse = "\n"), qmd, fixed = TRUE)
  writeLines(enc2utf8(qmd), file.path(work, "dashboard.qmd"), useBytes = TRUE)
  processx::run(
    quarto,
    c("render", "dashboard.qmd", "--to", "dashboard", "--quiet"),
    wd = work,
    timeout = 180000,
    cleanup_tree = TRUE
  )
  output <- file.path(work, "download.html")
  inline_resources(file.path(work, "dashboard.html"), output)
  readBin(output, "raw", n = file.size(output))
}

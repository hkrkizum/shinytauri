box::use(. / dice[DiceDistribution], .. / types[typed])

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

#' Inline the text resources of a self-contained Quarto HTML document
#' @param input Path to the rendered HTML.
#' @param output Path to the standalone HTML to write.
#' @return Invisible NULL after writing the document.
#' @export
inline_resources <- function(input, output) {
  # libxml2's HTML parser consumes end-tag-like JavaScript such as /</g.
  # Keep script bodies verbatim while editing Quarto's generated markup.
  html <- paste(
    readLines(
      typed(input, S7::class_character),
      encoding = "UTF-8",
      warn = FALSE
    ),
    collapse = "\n"
  )
  bodies <- gregexpr(
    "(?is)<script\\b(?:[^>\"']++|\"[^\"]*\"|'[^']*')*>\\K.*?(?=</script\\s*>)",
    html,
    perl = TRUE
  )
  scripts <- regmatches(html, bodies)[[1L]]
  regmatches(html, bodies) <- list(as.character(seq_along(scripts)))
  doc <- xml2::read_html(
    html,
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
  for (node in xml2::xml_find_all(doc, "//script")) {
    uri <- xml2::xml_attr(node, "src")
    if (is.na(uri)) {
      xml2::xml_text(node) <- scripts[[as.integer(xml2::xml_text(node))]]
    } else {
      stopifnot(grepl("^data:(application|text)/javascript[,;]", uri))
      xml2::xml_attr(node, "src") <- NULL
      xml2::xml_text(node) <- decode_uri(uri)
    }
  }
  xml2::write_html(
    doc,
    typed(output, S7::class_character),
    encoding = "UTF-8",
    options = "as_html"
  )
}

#' Render a snapshot as a standalone Quarto dashboard
#' @param counter Integer counter value at the time of the request.
#' @param counts Integer frequencies for sums two through twelve.
#' @param quarto Path to the Quarto executable.
#' @return Raw bytes of the standalone HTML file.
#' @export
dashboard <- function(counter, counts, quarto) {
  typed(counter, S7::class_integer)
  stopifnot(length(counter) == 1L, !is.na(counter), counter >= 0L)
  distribution <- DiceDistribution(counts = counts)
  typed(quarto, S7::class_character)
  work <- tempfile("dice-report-")
  stopifnot(dir.create(work))
  on.exit(unlink(work, recursive = TRUE), add = TRUE)
  saveRDS(
    list(
      counter = counter,
      counts = distribution@counts,
      root = normalizePath(box::file("../.."))
    ),
    file.path(work, "snapshot.rds")
  )
  stopifnot(file.copy(box::file("../report/dashboard.qmd"), work))
  processx::run(
    quarto,
    c("render", "dashboard.qmd", "--to", "dashboard", "--quiet"),
    env = c("current", QUARTO_R = R.home("bin")),
    wd = work,
    timeout = 180000,
    cleanup_tree = TRUE
  )
  output <- file.path(work, "download.html")
  inline_resources(file.path(work, "dashboard.html"), output)
  readBin(output, "raw", n = file.size(output)) |> typed(S7::class_raw)
}

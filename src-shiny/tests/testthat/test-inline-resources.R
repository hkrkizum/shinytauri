box::use(.. / .. / app / logic / report)

testthat::test_that("styles, nested CSS imports and modules are embedded as text", {
  src <- tempfile(fileext = ".html")
  dest <- tempfile(fileext = ".html")
  uri <- function(css) paste0("data:text/css,", curl::curl_escape(css))
  css <- paste0("@import url('", uri("p{color:red}"), "') print;")
  inline_script <- 'const escape = s => s.replace(/</g, "&lt;");'
  writeLines(
    paste0(
      "<html><head><link rel='stylesheet' href='",
      uri(css),
      "'>",
      "<script type='module' src='data:text/javascript;base64,",
      base64enc::base64encode(charToRaw("window.test = true;")),
      "'></script><script>",
      inline_script,
      "</script></head><body></body></html>"
    ),
    src
  )
  report$inline_resources(src, dest)
  testthat::expect_true(any(grepl(
    inline_script,
    readLines(dest),
    fixed = TRUE
  )))
  doc <- xml2::read_html(dest)
  style <- xml2::xml_text(xml2::xml_find_first(doc, "//style"))
  testthat::expect_match(style, "@media print { p{color:red} }", fixed = TRUE)
  testthat::expect_length(
    xml2::xml_find_all(doc, "//link[@href] | //script[@src]"),
    0
  )
  testthat::expect_identical(
    xml2::xml_text(xml2::xml_find_first(doc, "//script[@type='module']")),
    "window.test = true;"
  )
})

testthat::test_that("a report rejects an invalid snapshot before starting Quarto", {
  testthat::expect_error(report$dashboard("3", 1:11, "unused"), "integer")
  testthat::expect_error(
    report$dashboard(3L, c(-1L, 1:10), "unused"),
    "non-negative"
  )
})

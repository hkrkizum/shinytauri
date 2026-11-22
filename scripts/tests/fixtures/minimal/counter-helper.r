format_counter <- function(count) {
  sprintf(
    "%s (stringr %s)",
    stringr::str_pad(as.integer(count), 4, pad = "0"),
    utils::packageVersion("stringr")
  )
}

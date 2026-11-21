#' Check a value at a module or library boundary
#' @param value The value to check.
#' @param type The expected S7 class or S3 class specification.
#' @return The original value with its validated type.
#' @export
typed <- function(value, type) {
  if (!S7::S7_inherits(value, type)) {
    stop("Expected ", S7::S7_class_desc(type), call. = FALSE)
  }
  value
}

#' Shiny reactive expressions and reactive values.
#' @export
Reactive <- S7::new_S3_class("reactive")

#' A single HTML tag produced by a Shiny module.
#' @export
Tag <- S7::new_S3_class("shiny.tag")

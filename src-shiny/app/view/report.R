box::use(
  bslib[bind_task_button, card, card_body, card_header, input_task_button],
  mirai[mirai],
  shiny[
    conditionalPanel,
    downloadButton,
    downloadHandler,
    ExtendedTask,
    moduleServer,
    NS,
    observeEvent,
    outputOptions,
    reactive,
    renderText,
    req,
    tags,
    textOutput
  ]
)

box::use(.. / types[typed, Tag, Reactive])

box::use(. / distribution[distribution_table])

#' Display the distribution table and report download controls
#' @param id Shiny module ID.
#' @return A Bootstrap card.
#' @export
ui <- function(id) {
  ns <- NS(typed(id, S7::class_character))
  download <- downloadButton(ns("download"), "ダウンロード")
  download$attribs$target <- "_self"
  card(
    card_header(tags$h2(class = "h5 my-1", "集計レポート")),
    card_body(
      conditionalPanel(
        "!output.available",
        ns = ns,
        "サイコロを振るとレポートを作成できます。"
      ),
      conditionalPanel(
        "output.available",
        ns = ns,
        input_task_button(
          ns("render"),
          "レポートを作成",
          label_busy = "作成中…"
        )
      ),
      tags$div(`aria-live` = "polite", textOutput(ns("status"))),
      conditionalPanel(
        "output.ready",
        ns = ns,
        download
      ),
      DT::DTOutput(ns("table"), fill = FALSE)
    )
  ) |>
    typed(Tag)
}

#' Generate downloadable reports from completed simulations
#' @param id Shiny module ID.
#' @param distribution Reactive expression containing the current DiceDistribution.
#' @param counter Reactive expression containing the current integer count.
#' @return Invisible NULL after registering the session outputs.
#' @export
server <- function(id, distribution, counter) {
  moduleServer(
    typed(id, S7::class_character),
    function(input, output, session) {
      typed(distribution, Reactive)
      typed(counter, Reactive)
      output$table <- DT::renderDT(
        distribution_table(req(distribution())),
        server = FALSE
      )
      root <- normalizePath(box::file("../.."))
      task <- ExtendedTask$new(function(value, counts) {
        mirai(
          {
            options(box.path = root)
            box::use(app / logic / report)
            report$dashboard(value, counts, quarto)
          },
          root = root,
          value = value,
          counts = counts,
          quarto = Sys.getenv("SHINYTAURI_QUARTO", unset = Sys.which("quarto"))
        )
      }) |>
        bind_task_button("render")
      observeEvent(input$render, {
        task$invoke(counter(), req(distribution())@counts)
      })
      output$available <- reactive(!is.null(distribution()))
      output$ready <- reactive(identical(task$status(), "success"))
      outputOptions(output, "available", suspendWhenHidden = FALSE)
      outputOptions(output, "ready", suspendWhenHidden = FALSE)
      output$status <- renderText(switch(
        task$status(),
        running = "レポートを作成しています。",
        success = "作成時点の集計レポートをダウンロードできます。",
        error = "作成に失敗しました。もう一度レポートを作成してください。",
        ""
      ))
      output$download <- downloadHandler(
        filename = "dice-dashboard.html",
        contentType = "text/html; charset=utf-8",
        content = function(file) writeBin(task$result(), file)
      )
      invisible(NULL)
    }
  )
}

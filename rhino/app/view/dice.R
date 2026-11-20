box::use(
  bslib[bind_task_button, card, card_body, card_header, input_task_button],
  graphics[axis, barplot, lines, par],
  mirai[mirai],
  shiny[
    ExtendedTask,
    moduleServer,
    NS,
    observeEvent,
    plotOutput,
    renderPlot,
    renderText,
    req,
    selectInput,
    tags,
    textOutput
  ]
)

trials <- c("1,000 回" = 1000L, "10,000 回" = 10000L, "100,000 回" = 100000L)

#' @export
ui <- function(id) {
  ns <- NS(id)
  card(
    card_header(tags$h2(class = "h5 my-1", "サイコロのシミュレーション")),
    card_body(
      tags$p("2個のサイコロの出目を合計し、その分布を理論値と比べます。"),
      tags$div(
        class = "d-flex flex-wrap align-items-end gap-3",
        selectInput(
          ns("trials"),
          "試行回数",
          trials,
          selectize = FALSE,
          width = "10rem"
        ),
        input_task_button(
          ns("roll"),
          "サイコロを振る",
          label_busy = "集計中…",
          class = "mb-3"
        )
      ),
      plotOutput(ns("distribution"), height = "180px", fill = FALSE),
      tags$small(
        class = "text-body-secondary",
        "棒は実測値、折れ線は理論値です。"
      ),
      tags$div(`aria-live` = "polite", textOutput(ns("summary")))
    )
  )
}

#' @export
server <- function(id) {
  moduleServer(id, function(input, output, session) {
    task <- ExtendedTask$new(function(n) {
      mirai(
        {
          # 集計中にカウンターを試せるよう、デモ用の待ち時間を設ける。
          Sys.sleep(2)
          sums <- sample.int(6L, n, replace = TRUE) +
            sample.int(6L, n, replace = TRUE)
          tabulate(sums - 1L, nbins = 11L)
        },
        n = n
      )
    }) |>
      bind_task_button("roll")

    observeEvent(input$roll, {
      req(input$trials %in% trials)
      task$invoke(as.integer(input$trials))
    })

    output$distribution <- renderPlot(
      {
        sums <- 2:12
        expected <- (6 - abs(sums - 7)) / 36 * 100
        observed <- if (task$status() == "initial") {
          numeric(11)
        } else {
          prop.table(task$result()) * 100
        }
        par(mar = c(2, 3, 0.5, 0.5), fg = "#536963", col.axis = "#435c56")
        positions <- barplot(
          observed,
          names.arg = sums,
          ylim = c(0, max(20, observed)),
          col = "#248879",
          border = NA,
          axes = FALSE
        )
        ticks <- seq(0, max(20, observed), by = 5)
        axis(2, at = ticks, labels = paste0(ticks, "%"), las = 1, tick = FALSE)
        lines(positions, expected, type = "b", pch = 16, col = "#253d38")
      },
      alt = "サイコロ2個の出目の合計ごとに、実測と理論の出現率を示すグラフ"
    )

    output$summary <- renderText({
      if (task$status() == "initial") {
        return("試行回数を選んでサイコロを振ってください。")
      }
      counts <- task$result()
      sprintf(
        "%s 回のうち、合計が 7 になった割合は %.1f%% でした。",
        format(sum(counts), big.mark = ","),
        100 * counts[6L] / sum(counts)
      )
    })
  })
}

"""Thin CLI entry point: plot one suite's logs into figure CSVs + PDFs.

    python -m bigbird.plots.cli --input-dir <suite>/logs --output-dir <suite>
"""

from pathlib import Path

import typer

from bigbird.plots.figures import run

app = typer.Typer(pretty_exceptions_show_locals=False)


@app.command()
def main(input_dir: Path = None, output_dir: Path = None):
    run(input_dir, output_dir)


if __name__ == "__main__":
    app()

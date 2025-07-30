Contributing to Oxybox
-----

## What You'll Need

  * **Git**: Essential for version control. You can find installation instructions [here](https://git-scm.com/).
  * **A GitHub Account**: If you don't have one, you can sign up for free [here](https://github.com/).

-----

## Getting Started with Your Development Environment

To make setup as smooth as possible, you have a few options for your development environment:

  * **Devbox**: This is a pre-configured development environment that ensures you have all the necessary tools and dependencies readily available. [Link to Devbox setup instructions]
  * **Devcontainer**: If you use VS Code or another IDE that supports Devcontainers, you can get up and running quickly with a consistent environment.

If you prefer to set up your environment manually, follow these steps:

1.  **Fork the Repository**: Go to `https://github.com/baseflow/oxybox` and fork the repository to your personal GitHub account.
2.  **Clone Your Fork**: On your local machine, clone your forked repository:
    ```bash
    git clone git@github.com:<your_name_here>/oxybox.git
    ```
3.  **Navigate to the Project Directory**:
    ```bash
    cd oxybox
    ```
4.  **Add Upstream Remote**: To keep your fork synchronized with the original Oxybox repository, add it as an upstream remote:
    ```bash
    git remote add upstream https://github.com/baseflow/oxybox.git
    ```

-----

## Contributing to Oxybox

We highly appreciate contributions via GitHub pull requests\! Here's how to contribute:

1.  **Sync with Upstream**: Before you start, make sure your local `main` branch is up-to-date with the latest changes from the original repository:

    ```bash
    git fetch upstream
    git checkout main
    git merge upstream/main
    ```

2.  **Create a New Branch**: Create a new branch for your changes. Use a descriptive name that reflects the purpose of your contribution:

    ```bash
    git checkout -b <your-feature-or-fix-branch-name>
    ```

3.  **Make Your Changes**: Implement your features or bug fixes.

4.  **Test Your Changes**: Since Oxybox is written in Rust, you'll want to run tests to ensure your changes haven't introduced any regressions and work as expected.

    ```bash
    cargo test
    ```

5.  **Format Your Code**: Ensure your code adheres to the project's formatting standards:

    ```bash
    cargo fmt
    ```

6.  **Lint Your Code**: Run clippy to catch common mistakes and improve code quality:

    ```bash
    cargo clippy
    ```

7.  **Commit Your Changes**: Stage and commit your changes with a clear and concise commit message:

    ```bash
    git add .
    git commit -m "feat: Add a concise description of your changes"
    ```

    (Replace "feat:" with "fix:", "docs:", "refactor:", etc., as appropriate following conventional commits.)

8.  **Push to Your Fork**: Push your new branch to your forked repository on GitHub:

    ```bash
    git push origin <your-feature-or-fix-branch-name>
    ```

9.  **Create a Pull Request**: Go to the Oxybox repository on GitHub (`https://github.com/baseflow/oxybox`) and click the "Compare & pull request" button.

    Please ensure you:

      * Fill out the pull request template completely.
      * Address any warnings or errors reported by `cargo clippy` and `cargo test`.

We're excited to see your contributions\!

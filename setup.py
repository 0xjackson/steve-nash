from setuptools import setup, find_packages

setup(
    name="gto-cli",
    version="1.0.0",
    packages=find_packages(),
    install_requires=[
        "click>=8.0",
        "rich>=13.0",
    ],
    entry_points={
        "console_scripts": [
            "gto=gto.cli:main",
        ],
    },
    python_requires=">=3.10",
)

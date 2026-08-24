<div align="center">

# Nara.io

<p align=center>
    <sub>A 2D multiplayer shooter game, your objective is to dominate other tanks!</sub>
    <br />
    <sub>Heavily inspired by diep.io, arras.io, and havre.io!</sub>
</p>

[![Rust](https://img.shields.io/badge/Language-Rust-000000.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![WebAssembly](https://img.shields.io/badge/Target-WebAssembly-654FF0.svg?style=flat-square&logo=webassembly)](https://webassembly.org/)
[![License](https://img.shields.io/badge/License-MIT-yellow.svg?style=flat-square)](LICENSE)
![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg?style=flat-square)
![Code Size](https://img.shields.io/github/languages/code-size/gurmehar-singh-2009/araria?style=flat-square)
![Platform](https://img.shields.io/badge/platform-Web-lightgrey?style=flat-square)
[![Stand With Ukraine](https://raw.githubusercontent.com/vshymanskyy/StandWithUkraine/main/badges/StandWithUkraine.svg)](https://stand-with-ukraine.pp.ua)

</div>

---
> [!NOTE]
> **Nara.io is still under active development!** More features are to come.


## Features:

- Advanced Anti Cheat utilising:
- - Packet schema obfuscation.
- - Behavioural anti cheat scoring system.
- Lua Plugin support!
- - Ability to dispatch and receive events.
- - Ability to register administrator commands.
- Advanced GPU rendering using WebGPU (thanks to the wgpu crate!).


## Live Demo

---
> [!WARNING]
> The live demonstration is not a main priority, and therefore is not obligated to stay up to date with the repository.
> It is encouraged for you to clone the repository and host it yourself to play with your friends!

A live demo of the project can be found [here](https://page.naraioserver.hackclub.app).


## Getting Started

You will need the following installed:
- Rust
- Trunk

To install trunk:
```bash
cargo install trunk
```

Then you can run
```bash
chmod +x run_client.sh run_server.sh

# run this in one terminal
./run_client.sh

# run this in another
./run_server.sh
```
if you are hosting locally, otherwise use the `prod` variation.


## Crates Usage

I will refrain from importing unnecessary crates, or crates that I feel like I could replicate and improve functionality on. However, crates will be imported when:
- They are practically necessary for development.
- It will take too much time/effort to write myself.
- Writing it myself is not considered worth it.


## Note on Cryptography

This project utilizes an [X25519](https://cryptography.io/en/latest/hazmat/primitives/asymmetric/x25519/) handshake that establishes a [ChaCha20Poly1305](https://en.wikipedia.org/wiki/ChaCha20-Poly1305) cipher.
Unfortunately, this project will not be utilizing MlKem, however you may check out Roamer.io (link pending).


## Unique Features

- Lua scripting. You can create plugins that can interact with the game, as well as change configs at runtime.
- Anti cheat. More specifically: a behavioural anti cheat!


## Why use Rust? Why not write in JS/TS, or something else like that?

Rust is incredible. I love the syntax, ecosystem, and overall development cycle.
I recognize that you may be able to prototype faster in a language like TypeScript, but for me it is not as rewarding. I understand that AI is excellent at TypeScript, and it can essentially 1-shot TypeScript code.

In addition, Rust comes with performance buffs by being a lower level language with no Garbage Collector.

And overall, I believe Rust is the future for technological development. A language that prevents a whole suite of bugs at compile time is what everyone dreams of!


## Contribution Guidelines

I will not be accepting any feature contributions. Only contributions that refactor existing code with clear, listed out benefits. Final decisions will be made by me solely on what code is accepted, and I reserve the right to use my own discretion regardless.


## AI Usage

AI was used for converting formats like JSON to TOML, and some minor feature suggestions.
I refuse to use it for anything else, since I want to use this project as a learning opportunity.

However, I did use it to debug certain sections of my code.
Below is a list I will keep updated with what files I have used AI in:

- /client/src/render/shader/fragment.wgsl
- /client/bin/ - Not technically in use, experimentation.

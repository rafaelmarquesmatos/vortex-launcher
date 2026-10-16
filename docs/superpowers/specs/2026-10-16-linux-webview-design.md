# Linux webview launcher

v1 do Vortex Launcher no Linux: uma janela Tauri 2 que abre `https://playvortex.io`.

## Fora desta v1

- Wine e download do client Windows
- Protocolo `vortex://`
- Bandeja do sistema, atalho e updater
- UI local, tela de loading e tela de erro própria
- Pastas para Windows ou macOS

## Layout

```text
linux/
  package.json
  src-tauri/
    Cargo.toml
    tauri.conf.json
    build.rs
    capabilities/default.json
    src/lib.rs
    src/main.rs
    icons/
```

A raiz do repositório continua sendo o git. `linux/` é o app.

## Stack

Bun só executa `@tauri-apps/cli`. Não há Vite, bundler nem frontend local. Tauri 2.

`beforeDevCommand` e `beforeBuildCommand` ficam vazios.

## Configuração

| Campo | Valor |
| --- | --- |
| `productName` | `Vortex Launcher` |
| `version` | `0.1.0` |
| `identifier` | `io.playvortex.launcher` |
| `build.devUrl` | `https://playvortex.io` |
| `build.frontendDist` | `https://playvortex.io` |

`frontendDist` como URL remota não embute assets. Dev e build carregam o mesmo endereço.

Janela principal:

- título `Vortex`
- 1280×800
- redimensionável
- centralizada

Ícones padrão gerados pelo Tauri. Sem ícone próprio nesta v1.

## Segurança

O site não recebe IPC.

- Não definir `dangerousRemoteDomainIpcAccess`.
- `withGlobalTauri` desligado.
- A capability padrão não amplia acesso para a origem remota.

Navegação livre dentro da webview, inclusive redirects de login.

## Erro

Se o site não carregar, a webview mostra a página nativa de erro. O app não intercepta essa falha.

## Scripts

Em `linux/`:

- `bun run dev` executa `tauri dev`
- `bun run build` executa `tauri build`

O projeto não instala as bibliotecas de sistema do Tauri. Elas precisam já estar na máquina.

## Aceite

- `bun run dev` abre uma janela em `https://playvortex.io`.
- O binário gerado por `bun run build` abre o mesmo URL.
- A página remota não tem API Tauri.

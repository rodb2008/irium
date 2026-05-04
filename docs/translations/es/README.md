<!-- AI Translation Notice -->
> Nota: Esta traducción fue generada por IA y aún no ha sido revisada por un hablante nativo. Las correcciones y mejoras son bienvenidas mediante pull request.

# Irium Blockchain (Mainnet en Rust)

[![Rust](https://img.shields.io/badge/Rust-Blockchain-orange?logo=rust)](https://www.rust-lang.org/)
[![Algoritmo](https://img.shields.io/badge/Algoritmo-SHA256d-blue)](https://github.com/iriumlabs/irium)
[![Consenso](https://img.shields.io/badge/Consenso-Prueba--de--Trabajo-green)](https://github.com/iriumlabs/irium)
[![Licencia](https://img.shields.io/badge/Licencia-MIT-lightgrey)](https://github.com/iriumlabs/irium/blob/main/LICENSE)

## Irium (IRM)

Irium es una **blockchain de prueba de trabajo exclusivamente para producción** para el activo IRM.

La red se lanza con:

- Sin red de pruebas
- Sin dependencia de DNS (bootstrap con lista de semillas firmada)
- Génesis bloqueado que aplica el calendario de adquisición del fundador
- Suministro fijo máximo de **~24,500,000 IRM**

Este repositorio contiene la **implementación en Rust del nodo completo, minero, herramientas de billetera y utilidades SPV**.

---

### Consenso

- Algoritmo: SHA-256d
- Objetivo de tiempo de bloque: 600 segundos
- Reajuste de dificultad: cada 2016 bloques hasta la activación LWMA en el bloque 16,462, luego LWMA
- Subsidio inicial: 50 IRM
- Intervalo de reducción a la mitad: 210,000 bloques
- Madurez de coinbase: 100 bloques
- Suministro máximo: ~24,500,000 IRM
- Asignación génesis: **3,500,000 IRM bloqueados con CLTV**

---

### Bootstrap

El descubrimiento de pares utiliza:

- `bootstrap/seedlist.txt` firmado
- `anchors.json`
- Pares en caché en `bootstrap/seedlist.runtime`

---

### Objetivos de Diseño

- Arquitectura con mainnet como prioridad
- Bootstrap sin DNS
- Amigable con clientes ligeros
- Recompensas de retransmisión opcionales

## ¿Por qué minar Irium?

• Red de prueba de trabajo en etapa muy temprana
• Blockchain independiente en Rust (no es un fork)
• Arquitectura de descubrimiento de pares sin DNS
• Distribución de lanzamiento transparente — sin ICO, sin preventa, sin airdrop; 3,500,000 IRM de adquisición génesis bloqueados en cadena

---

# Enlaces Rápidos

Sitio web: https://iriumlabs.org

Explorador: https://www.iriumlabs.org/explorer

Pool de minería: pool.iriumlabs.org (3333 para ASIC, 3335 para CPU/GPU)

Bitcointalk ANN: https://bitcointalk.org/index.php?topic=5572239.0

Telegram: https://t.me/iriumlabs

Organización GitHub: https://github.com/iriumlabs

---

# Minar Irium (Forma más rápida)

### 1. Instalar Rust

Visita https://rustup.rs para instalar Rust. Abre una nueva terminal después de la instalación.

### 2. Descargar el Código Fuente

```bash
git clone https://github.com/iriumlabs/irium.git
cd irium
```

### 3. Compilar el Software

```bash
source ~/.cargo/env
cargo build --release
```

### 4. Iniciar el Nodo

```bash
./target/release/iriumd
```

Deja esta ventana ejecutándose.

### 5. Crear una Dirección de Billetera

Abre una segunda terminal:

```bash
./target/release/irium-wallet init
./target/release/irium-wallet new-address
```

Copia la dirección generada.

### 6. Comenzar a Minar

```bash
export IRIUM_MINER_ADDRESS=<TU_DIRECCIÓN>
./target/release/irium-miner
```

La minería comenzará una vez que el nodo esté sincronizado.

---

# Verificar Tu Saldo

```bash
./target/release/irium-wallet balance <TU_DIRECCIÓN>
```

---

# Ejecutar un Nodo

```bash
./target/release/iriumd
```

Directorios predeterminados:
- Bloques: `~/.irium/blocks`
- Estado: `~/.irium/state`

---

# Solución de Problemas

Minero atascado en bloque 0 → el nodo aún está sincronizando

El minero no puede obtener plantillas de bloque → verifica la conexión RPC

Sin pares → asegúrate de que el puerto TCP **38291** saliente esté permitido

HTTP 401 → configura `IRIUM_RPC_TOKEN` coincidente para el nodo y el minero

---

# Licencia

Licencia MIT

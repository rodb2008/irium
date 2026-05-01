<!-- AI Translation Notice -->
> Nota: Esta traducción fue generada por IA y aún no ha sido revisada por un hablante nativo. Las correcciones y mejoras son bienvenidas mediante pull request.

# Tutorial de Liquidación de Irium: Cobrar con Seguridad como Freelancer

## El Escenario

Alice es desarrolladora freelance. Bob quiere contratarla para construir un sitio web por 50 IRM.

Nunca han trabajado juntos. Alice no sabe si Bob pagará después de que ella entregue el trabajo. Bob no sabe si Alice entregará después de que él pague. Ninguno quiere dar el primer paso.

La liquidación de Irium resuelve esto con un depósito en garantía en cadena. Bob bloquea 50 IRM en un contrato antes de que Alice haga ningún trabajo. Alice sabe que el dinero está ahí y Bob no puede moverlo hasta que el acuerdo se resuelva. Si Alice entrega y la prueba es aceptada, ella recibe el pago. Si pasa el plazo sin entrega, Bob recupera automáticamente su dinero.

Sin banco. Sin abogado. Sin necesidad de confianza.

---

## Paso 1 — Alice y Bob acuerdan los términos fuera de la cadena

Antes de que nada vaya a la cadena, acuerdan:

- El monto: 50 IRM
- Qué cuenta como entrega: un sitio web funcional en una URL acordada
- Un plazo (expresado como altura de bloque, aproximadamente 1 bloque por 10 minutos)
- Un documento breve con la descripción del trabajo que ambos firman

Escriben su acuerdo en un archivo `terms.txt`. Este archivo se codificará en hash y se registrará en cadena para que ninguna de las partes pueda afirmar posteriormente que los términos eran diferentes.

---

## Paso 2 — Generar un secreto y crear el acuerdo

Bob genera un secreto de un solo uso. Este es un número aleatorio que desbloqueará los fondos cuando se revele:

```bash
# Generar un secreto aleatorio de 32 bytes (Bob lo mantiene privado hasta estar satisfecho)
SECRET=$(openssl rand -hex 32)

# Calcular el hash del secreto (esto va en el acuerdo, no el secreto en sí)
SECRET_HASH=$(printf '%s' "$SECRET" | xxd -r -p | sha256sum | awk '{print $1}')
```

Bob hace hash del documento de términos para vincularlo al registro en cadena:

```bash
DOCUMENT_HASH=$(sha256sum terms.txt | awk '{print $1}')
```

Bob crea el JSON del acuerdo:

```bash
irium-wallet agreement-create-simple-settlement \
  --agreement-id website-project-001 \
  --creation-time $(date +%s) \
  --party-a "id=alice,name=Alice,role=freelancer" \
  --party-b "id=bob,name=Bob,role=client" \
  --amount 50 \
  --secret-hash $SECRET_HASH \
  --refund-timeout 21500 \
  --document-hash $DOCUMENT_HASH \
  --release-summary "Alice entrega el sitio web completado antes del bloque 21500" \
  --refund-summary "Bob recupera fondos si el sitio web no se entrega antes del bloque 21500" \
  --out website-project-001.json
```

---

## Paso 3 — Compartir el acuerdo con Alice

Bob envía el archivo JSON a Alice. Alice lo inspecciona:

```bash
irium-wallet agreement-inspect website-project-001.json
```

Alice verifica: monto, plazo, descripción de entrega. Si está conforme, confirma.

---

## Paso 4 — Bob financia el depósito en garantía (el dinero va a la cadena)

```bash
irium-wallet agreement-fund website-project-001.json \
  --broadcast \
  --rpc http://localhost:38300
```

Los 50 IRM quedan bloqueados. Bob no puede recuperarlos antes del bloque 21500. Alice confirma que los fondos están esperando en la cadena.

---

## Paso 5 — Alice realiza el trabajo

Alice construye el sitio web. Cuando está listo, notifica a Bob para su revisión.

---

## Paso 6 — Bob acepta y revela el secreto

Si Bob está satisfecho con el trabajo, le da a Alice el secreto:

```bash
# Bob envía este valor a Alice (de forma privada, por ejemplo, por Telegram)
echo "Mi secreto: $SECRET"
```

Alice usa el secreto para verificar elegibilidad y reclamar los fondos:

```bash
irium-wallet agreement-release-eligibility website-project-001.json \
  --secret $SECRET \
  --destination <DIRECCIÓN_ALICE> \
  --rpc http://localhost:38300
```

Los 50 IRM se transfieren a la dirección de Alice. El acuerdo queda liquidado.

---

## ¿Qué pasa si Bob no acepta?

**Escenario A — Bob retiene el secreto a pesar de la buena entrega**

Alice puede enviar una prueba a la red Irium. Un atestador designado revisa la evidencia y, si la prueba cumple con la política, el atestador publica el secreto de liberación.

**Escenario B — El tiempo de espera expira**

Pasa el bloque 21500. Bob no ha aceptado ni disputado. El depósito en garantía se vuelve automáticamente reembolsable a Bob.

```bash
irium-wallet agreement-refund-eligibility website-project-001.json \
  --destination <DIRECCIÓN_BOB> \
  --rpc http://localhost:38300
```

---

## Resumen de Resultados

| Situación | Resultado |
|-----------|-----------|
| Alice entrega, Bob acepta | Alice recibe 50 IRM |
| Alice entrega, Bob retiene el secreto | Alice puede enviar prueba; atestador puede liberar fondos |
| Alice no entrega | Bob recupera 50 IRM automáticamente después del tiempo de espera |
| Bob no responde después del tiempo de espera | Bob recupera 50 IRM automáticamente |
| Bob intenta recuperar fondos anticipadamente | Imposible — el contrato HTLC lo previene |

---

Versión en inglés: [docs/SETTLEMENT-EXAMPLE.md](../../SETTLEMENT-EXAMPLE.md)

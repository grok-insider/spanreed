# Experimentos de capacidades de Grok Remote

Estado: implementados para evaluación, desactivados por defecto. Las pruebas
nativas no sustituyen la comprobación de la PWA contra un host desplegado.

## Activación

El host lee las variables al arrancar; solo `1` o `true` habilitan una opción.

| Variable | Capacidad |
| --- | --- |
| `SPANREED_REMOTE_EXPERIMENT_PROJECTS` | Navegar y registrar proyectos bajo raíces autorizadas localmente; solo Unix |
| `SPANREED_REMOTE_EXPERIMENT_CONTROL_TRANSFER` | Solicitud y cesión explícita del control entre conexiones |
| `SPANREED_REMOTE_EXPERIMENT_DEVICE_PANEL` | Enumerar permisos de navegador y conexiones activas |

`getRemoteCapabilities` devuelve las tres capacidades efectivas. `projects`
requiere además un directorio de estado persistente. La transferencia exige
el modo ACP compartido; el modo aislado no la anuncia ni admite observadores
por activar esta variable. `upstreamPrompt` es nulo
hasta una inicialización ACP correcta; después contiene exclusivamente los
booleanos declarados por el agente en `agentCapabilities.promptCapabilities`.
La ausencia o un tipo incorrecto significa `false`. `_meta` no los sustituye.
Son capacidades del agente, no una promesa de que el protocolo de la PWA ya
permita enviar imágenes o audio. Al terminar el agente se elimina esta lectura.

## Proyectos

El canal local exclusivo del dueño acepta `authorizeRemoteRoot {path}`,
`listRemoteRoots` y `revokeRemoteRoot {rootId}`. Su respuesta `remoteRoots`
incluye la ruta y la identidad de filesystem para el CLI local. El catálogo
`remote-roots.json` se reemplaza atómicamente, con permisos `0600` en Unix,
y se carga antes de servir. Una corrupción impide arrancar con autorización
inventada. Se admiten hasta 32 raíces.

El navegador usa `listRemoteProjects {directoryId?,cursor?}`. Sin identificador
recibe las raíces; con uno recibe los subdirectorios. Cada entrada contiene
solo `directoryId` y `label`. `parentId` identifica el directorio de la petición,
no su padre: el cliente mantiene la navegación. Las páginas tienen hasta 100
entradas. El cursor se puede repetir hasta su caducidad y conserva la página.
No se leen recursivamente árboles completos; más de 4096 entradas de directorio
provocan un rechazo explícito. Hay límites de 1024 identificadores y 64 cursores.

Los identificadores aleatorios pertenecen a un permiso de navegador, duran
cinco minutos desde su última emisión y desaparecen al reiniciar. Las visitas
al mismo directorio e identidad reutilizan su identificador y las páginas sin
cambios reutilizan sus cursores. Las raíces desaparecidas no consumen
identificadores y las etiquetas vacías reciben el nombre relativo «Directory». La revocación de una raíz invalida
sus identificadores y los cursores existentes. La apertura se realiza desde
un descriptor de la raíz con `cap-std`, se rechazan enlaces simbólicos y se
comprueban dispositivo e inodo de raíz y destino. El navegador no puede enviar
una ruta libre.

`enrollRemoteProject {directoryId}` exige control e idempotencia. Se registra
la identidad del directorio abierto y se vuelve a comparar la ruta antes de
persistir; nunca se sustituye silenciosamente por la de otro directorio. La
respuesta reutiliza `workspaces` y se emite `workspacesChanged`. Las escrituras
del índice desde este host se serializan con el selector y la revocación.
Con el experimento habilitado, las operaciones que usan un workspace vuelven
a validar el índice antes de despachar.

Revocar una raíz detiene futuras navegaciones y registros. Los workspaces que
ya se registraron tienen su autorización independiente y se eliminan con la
operación de workspace. El cwd de ACP sigue siendo una ruta: estas guardas no
son un aislamiento frente a otro proceso local que cambie directorios durante
la ejecución del agente. Windows queda sin esta capacidad hasta disponer de
identidades de filesystem adecuadas.

## Control y dispositivos

Una conexión observadora usa `requestControlTransfer`; recibe
`controlTransferRequested {request:{requestId,sessionId,expiresAt}}`. La petición
caduca en 30 segundos (`expiresAt` es Unix en milisegundos). Solo esa conexión
puede cancelar mediante `cancelControlTransfer {requestId}`; otra pestaña del
mismo navegador no puede hacerlo. No hay toma forzosa del control.

El controlador aprueba con `approveControlTransfer {requestId}`, su token de
conexión y su epoch vigente. El host vuelve a comprobar que el destino sigue
conectado y su permiso existe, cambia atómicamente el lease e incrementa el
epoch. La respuesta solo confirma `requestId`; no entrega el secreto ni el
epoch del destinatario al controlador anterior. Cada canal recibe su propio
`hostStatus`: `controlling:<epoch>` o `watching`. Estas señales de autoridad no
se escriben en el journal global. Los comandos posteriores del controlador
anterior se rechazan; desconectar ese observador no interrumpe al nuevo dueño.
Un heartbeat antiguo que ya estaba en tránsito no cierra el canal observador.

`listBrowserDevices` devuelve permisos vigentes, `currentBrowser`,
`currentController`, `connected`, `connectionCount` y `lastSeen: null`. No hay
nombre de dispositivo ni fecha inventados, claves Noise, tokens o rutas.
Incluye `controlTransferRequests` con las peticiones vigentes. Esta lectura
también está disponible cuando solo se habilita la transferencia, porque el
controlador necesita ver sus solicitudes.

`remoteDevicesChanged`, sin payload, invalida esta lectura al conectar,
desconectar, solicitar, cancelar, aprobar o revocar. La interfaz usa
`expiresAt` para retirar peticiones caducadas; no existe otro sondeo periódico
del host solo para su caducidad. Las mutaciones nuevas requieren clave de
idempotencia y pasan por intención durable antes de su efecto.

## Evidencia local

En esta tanda se comprobaron las guardas puras de raíces, paginación, ámbito,
caducidad, enlaces, sustitución de directorios, revocación y reinicio. Las
pruebas de host ejercitan registro real con persistencia y rechazo de un
workspace sustituido antes de `createSession`. Las de control prueban epoch,
identidad de conexión, consentimiento, expiración y ausencia de secretos en
las proyecciones. La transferencia atraviesa también canales Noise reales
en memoria y sockets WebSocket locales en modo ACP compartido. El modo aislado
rechaza explícitamente la capacidad aunque esté activada su variable.

La salida WebSocket privada vuelve a comprobar el permiso entre eventos,
incluso cuando ya copió un lote del journal, y cada envío tiene un único límite
de cinco segundos. Una prueba con cola de 16 elementos llena permite como
máximo los 16 ya enviados y uno en tránsito después de revocar; quitar la
comprobación intermedia hace fallar la misma prueba. Una prueba adicional mide
50 cesiones alternadas mediante HTTP y WebSocket reales de loopback, verifica
50 rechazos de conexión anterior y otros 50 de epoch anterior, y exige p95 de
como máximo dos segundos. No incluye navegador, relay ni Tailscale.

Los experimentos de entrega de eventos y detección de publicaciones tienen
otro artefacto sellado, anterior a estas capacidades. Sus cifras no califican
este código nuevo. La entrega medida excluye relay, WebSocket de red, WASM y
renderizado. La publicación medida excluye HTTP/S3 y la planificación real.
Faltan CPU en reposo y wakeups reales; ninguna de las dos optimizaciones se
promueve por esas mediciones parciales.

## Cualificación del 13 de octubre de 2026

La suite final de `fabrials-agent-host` pasó 407 pruebas unitarias y 77 de
integración; `fabrials-remote-host`, 44 pruebas, con tres entradas optativas
excluidas de la suite ordinaria. Clippy de los tres crates nativos pasó con
todos los targets y `-D warnings`. La prueba de navegación incluye cadenas
explícitas de traversal y la de cesión hace competir dos solicitudes y sus
aprobaciones: exactamente una conexión acaba controlando. Una ejecución nueva
de las 50 cesiones por HTTP/WebSocket de loopback midió p95 de 0,720455 ms.
Este tiempo no incluye el relay ni la PWA.

`examples/qualify_shared_matrix.rs`, dirigido por
`scripts/qualify-shared-matrix.py`, pasó con los ejecutables oficiales 1.0.50 y
1.0.52. Usa dos clientes ACP contra un líder real, un workspace desechable,
Grok home y socket propios y la credencial `local-dev`. Comprueba el mismo
líder, prompt y aprobación en las dos direcciones, observadores, reconexión,
ausencia de repetición y pérdida del adaptador mientras el otro cliente sigue
trabajando. Cada versión produce exactamente tres efectos de archivo
autorizados. La prueba de CLI incompatible usa un ejecutable sintético que
rechaza el modo compartido antes de producir efectos. Esta matriz no abre una
TUI interactiva ni atraviesa Noise, relay o navegador.

`examples/qualify_syl_remote.rs`, dirigido por `scripts/qualify-syl-remote.py`,
pasó la cadena real `RemoteAccess → Grok 1.0.52 compartido → MCP Syl → navegador`
sobre una página sintética aislada. Se aprobaron ocho llamadas una sola vez;
las ocho tuvieron fin y una fue un rechazo real por observación obsoleta. El
servidor de prueba registró exactamente los dos efectos esperados. Se cortó la
conexión mientras un efecto esperaba su respuesta y se retomó la misma orden
sin repetirlo; la revocación por el socket del dueño rechazó órdenes y eventos
posteriores. Los nombres finales de Syl se comprueban en `ToolProgress`, porque
Grok publica primero `use_tool` en `ToolStart`. El navegador quedó cerrado al
terminar. No cubre Noise/PWA, Android, el escritorio real, reiniciar el host
durante un resultado incierto ni mostrar el cuerpo de las salidas de Syl en
Remote.

El evaluador central dio `keep` a `terminal-continuity` en ese alcance y
`inconclusive` a proyectos, cesión y Syl: pasan sus guardas técnicas, pero no
se midió el beneficio de la tarea ni participó un usuario en estas versiones.
El panel de dispositivos conserva comprobaciones pendientes de host/cuenta
en la interfaz; las pruebas nativas de revocación no sustituyen ese recorrido.
Ninguna capacidad experimental se activa por estos resultados.

La evidencia local está en
`/tmp/grok-remote-evaluation-ter17my1/native-evaluator-02/receipt.json`, con
inputs, resultados, catálogo, evaluador y hashes por comprobación. El manifiesto
nativo verifica 71 archivos de producción, pruebas y metadatos de compilación
idénticos al artefacto anterior a las ejecuciones; los ejemplos y ejecutables
de cada matriz se identifican aparte. Las cifras de rendimiento conservan sus
manifiestos originales en `native-evaluator-01`: no se atribuyen a la fuente
posterior. Estos artefactos temporales son evidencia de la estación, no una
publicación ni un resultado de CI.

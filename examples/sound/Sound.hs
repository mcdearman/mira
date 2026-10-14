{-# LANGUAGE ForeignFunctionInterface #-}

-- | Sound: a rising scale, a note at a time, and a ball that hums as it goes round you.
--
-- > cargo run --bin mira-play -- examples/sound
-- > MIRA_AUDIO=0 cargo run --bin mira-play -- examples/sound    # the same, with no sound out
--
-- The whole example is this file: the engine builds it, and builds it again when it is
-- saved. Change a pitch below while it plays and the hum changes.
module Sound where

import Control.Monad (when)
import Data.Int (Int32)
import Foreign (Ptr, Storable (..), castPtr)
import Foreign.C.Types (CInt (..))
import Mira

-- | The notes of the scale, in hertz, one every quarter of a second from half a second in.
scale' :: [Float]
scale' = [261.63, 293.66, 329.63, 349.23, 392.0, 440.0, 493.88, 523.25]

-- | How high the ball hums, how far off it can be heard, and how wide its circle is.
humPitch, humRange, circle :: Float
humPitch = 110
humRange = 12
circle = 4

-- | Marks the ball that hums; the number is how fast it goes round.
newtype Hummer = Hummer Float

instance Storable Hummer where
  sizeOf _ = 4
  alignment _ = 4
  peek ptr = Hummer <$> peek (castPtr ptr)
  poke ptr (Hummer speed) = poke (castPtr ptr) speed

foreign export ccall "mira_hs_main" pluginMain :: Ptr () -> IO CInt

pluginMain :: Ptr () -> IO CInt
pluginMain = plugin $ \app -> do
  transform <- lookupComponent app "mira.Transform"
  hummer <- registerComponent app "sound.Hummer"
  -- How many notes of the scale have been played: kept by the engine, so that saving this
  -- file while it plays does not begin the scale again.
  played <- statePtr app "sound.played" :: IO (Ptr Int32)

  addSystem_ app "setup" Startup $ \sys -> do
    -- A ball that hums, heard from where it is.
    hum <- soundTone sys humPitch 2
    ball <- spawn sys
    insert sys ball transform (at (V3 circle 1 0))
    insert sys ball hummer (Hummer 0.8)
    setMesh sys ball =<< meshSphere sys 0.3
    setMaterial sys ball (material (V3 0.9 0.6 0.2))
    setSoundSource sys ball hum 0.6 humRange True

    ground <- spawn sys
    insert sys ground transform (at (V3 0 0 0))
    setMesh sys ground =<< meshPlane sys 30
    setMaterial sys ground (material (V3 0.3 0.32 0.35))

    -- Where you stand: the camera, which is also the ears.
    eye <- spawn sys
    insert sys eye transform (lookingAt (V3 0 1.6 0) (V3 0 1 (-4)))
    setCamera sys eye defaultCamera
    setListener sys eye

    sun <- spawn sys
    insert sys sun transform (lookingAt (V3 0 0 0) (V3 (-0.5) (-1) (-0.35)))
    setLight sys sun defaultLight

  -- Each note is played when its time comes.
  addSystem_ app "tune" Update $ \sys -> do
    now <- realToFrac <$> elapsedSeconds sys
    next <- fromIntegral <$> peek played
    when (next < length scale' && 0.5 + fromIntegral next * 0.25 <= (now :: Float)) $ do
      note <- soundTone sys (scale' !! next) 0.22
      _ <- playSound sys note 0.5
      poke played (fromIntegral next + 1)

  addSystem app "circle" Update ((,) <$> write transform <*> readC hummer) $ \sys _ (place, Hummer speed) -> do
    now <- realToFrac <$> elapsedSeconds sys
    modifyRef place $ \t -> t {translation = V3 (cos (now * speed) * circle) 1 (sin (now * speed) * circle)}

at :: V3 -> Transform
at position = Transform position (Quat 0 0 0 1) (V3 1 1 1)